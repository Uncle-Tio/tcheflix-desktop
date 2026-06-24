//! UpdateBrowser business logic (Tchê Flix auto-updater dialog).
//!
//! Mirrors [`business_about`](crate::business_about): a self-managing singleton
//! CEF layer loading `app://resources/update.html`. The differences are that it
//! is opened from the background updater thread (so every entry point hops to
//! TID_UI via a posted task), it streams download progress into the page via
//! `exec_js`, and "Atualizar" fires a Rust apply-hook (installed by the main
//! app) before initiating a clean shutdown so the installer can take over.
//!
//! Decoupling: this is a new fork-owned file. The only shared-file touch points
//! are the `"update"` injection profile (injection.rs), the served HTML/JS
//! (resource.rs), and the `pub mod` line (lib.rs).

use cef::rc::Rc;
use cef::{
    ImplBrowser, ImplBrowserHost, ImplTask, Task, ThreadId, WrapTask, post_delayed_task, post_task,
    wrap_task,
};
use parking_lot::Mutex;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, OnceLock};

use crate::browsers::{jfn_browsers_create, jfn_browsers_set_active};
use crate::client::{
    Inner, jfn_cef_layer_create, jfn_cef_layer_inner, jfn_cef_layer_set_name,
    jfn_cef_layer_set_visible,
};
use crate::ipc::BrowserMessage;
use jfn_playback::shutdown::{jfn_shutdown_initiate, jfn_shutting_down};

static OPEN: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);
static PROGRESS: AtomicI32 = AtomicI32::new(-1);
/// Guards against spawning more than one focus-heartbeat loop at a time.
static HEARTBEAT_RUNNING: AtomicBool = AtomicBool::new(false);

/// Cadence of the focus heartbeat (see [`start_heartbeat`]). Short enough that
/// a focus steal is reclaimed before the user can notice the modal went
/// click-through, cheap enough to run for the dialog's whole lifetime.
const HEARTBEAT_MS: i64 = 500;

/// Live update layer's `Inner`, used to push JS (progress/ready) on TID_UI.
static UPDATE_INNER: Mutex<Option<Arc<Inner>>> = Mutex::new(None);

/// `(current_version, new_version, raw_markdown_notes)` shown by the dialog.
/// Read by resource.rs's `update_js_payload` when serving
/// `app://resources/update.js`.
static UPDATE_DATA: Mutex<Option<(String, String, String)>> = Mutex::new(None);

/// Installed by the main app: spawns the downloaded installer. Fired on
/// "Atualizar" just before shutdown.
static APPLY_HOOK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Install the apply-hook (called once at startup by the main app).
pub fn jfn_update_set_apply_hook(f: Box<dyn Fn() + Send + Sync>) {
    let _ = APPLY_HOOK.set(f);
}

fn fire_apply_hook() {
    if let Some(f) = APPLY_HOOK.get() {
        f();
    }
}

/// Snapshot of the data the dialog should render, for the resource handler.
/// The initial paint seeds itself from this (covers the case where progress or
/// ready arrived before the page finished loading); live `exec_js` pushes then
/// refine it.
pub(crate) struct UpdateData {
    pub current: String,
    pub version: String,
    pub notes: String,
    pub ready: bool,
    pub progress: i32,
}

pub(crate) fn current_update_data() -> Option<UpdateData> {
    let (current, version, notes) = UPDATE_DATA.lock().clone()?;
    Some(UpdateData {
        current,
        version,
        notes,
        ready: READY.load(Ordering::Acquire),
        progress: PROGRESS.load(Ordering::Acquire),
    })
}

// ---- public entry points (callable from any thread) ------------------------

/// Open the update dialog. `current` is the running version, `version` the
/// newer release being offered, and `notes` its raw markdown changelog.
pub fn jfn_update_open(current: String, version: String, notes: String) {
    *UPDATE_DATA.lock() = Some((current, version, notes));
    READY.store(false, Ordering::Release);
    PROGRESS.store(-1, Ordering::Release);
    post_ui(UiAction::Open);
}

/// Update the download progress bar (0..=100).
pub fn jfn_update_push_progress(pct: u8) {
    PROGRESS.store(i32::from(pct), Ordering::Release);
    post_ui(UiAction::Progress(i32::from(pct)));
}

/// Enable the "Atualizar" button — the installer finished downloading.
pub fn jfn_update_set_ready() {
    READY.store(true, Ordering::Release);
    post_ui(UiAction::Ready);
}

// ---- TID_UI task -----------------------------------------------------------

#[derive(Clone, Copy)]
enum UiAction {
    Open,
    Progress(i32),
    Ready,
    /// One tick of the focus heartbeat — reclaims input, then re-posts itself
    /// while the dialog stays open.
    Heartbeat,
}

fn post_ui(action: UiAction) {
    let mut task = UpdateUiTask::new(action);
    let _ = post_task(ThreadId::UI, Some(&mut task));
}

fn post_ui_delayed(action: UiAction, delay_ms: i64) {
    let mut task = UpdateUiTask::new(action);
    let _ = post_delayed_task(ThreadId::UI, Some(&mut task), delay_ms);
}

wrap_task! {
    struct UpdateUiTask {
        action: UiAction,
    }
    impl Task {
        fn execute(&self) {
            match self.action {
                UiAction::Open => open_on_ui(),
                UiAction::Progress(pct) => {
                    if let Some(inner) = live_inner() {
                        inner.exec_js(&format!(
                            "window._tcheflixSetProgress&&window._tcheflixSetProgress({pct});"
                        ));
                    }
                }
                UiAction::Ready => {
                    if let Some(inner) = live_inner() {
                        inner.exec_js("window._tcheflixSetReady&&window._tcheflixSetReady();");
                    }
                }
                UiAction::Heartbeat => heartbeat_tick(),
            }
        }
    }
}

/// Start the focus heartbeat if one isn't already running. The dialog is modal
/// and must own input for its whole lifetime, but an unguarded
/// `jfn_browsers_set_active(main)` elsewhere (notably the server overlay being
/// dismissed, which can land *after* the dialog opens) pops it off the active
/// stack, leaving it visible but click-through. The created-callback's initial
/// claim only fixes a steal that already happened; a steal that arrives later —
/// e.g. when the installer was already cached so there are no download ticks to
/// mask it — is never corrected. So re-claim input on a steady cadence until the
/// dialog closes. Must run on TID_UI.
fn start_heartbeat() {
    if HEARTBEAT_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    post_ui_delayed(UiAction::Heartbeat, HEARTBEAT_MS);
}

fn heartbeat_tick() {
    // Stop once the dialog is gone (dismissed / installing / shutting down).
    if jfn_shutting_down() || !OPEN.load(Ordering::Acquire) {
        HEARTBEAT_RUNNING.store(false, Ordering::Release);
        return;
    }
    reassert_active();
    post_ui_delayed(UiAction::Heartbeat, HEARTBEAT_MS);
}

/// The live update layer's `Inner`, if the dialog is open and its browser is
/// alive. Returns `None` during shutdown so no JS is pushed into a dying layer.
fn live_inner() -> Option<Arc<Inner>> {
    if jfn_shutting_down() || !OPEN.load(Ordering::Acquire) {
        return None;
    }
    let inner = UPDATE_INNER.lock().clone()?;
    inner.browser_alive().then_some(inner)
}

/// Re-assert the update layer as the active input target. Driven by the focus
/// heartbeat (see [`start_heartbeat`]). Idempotent: `jfn_browsers_set_active`
/// no-ops when the layer is already on top, so most ticks do nothing.
fn reassert_active() {
    if let Some(inner) = live_inner() {
        let p = inner.layer_ptr();
        if !p.is_null() {
            jfn_browsers_set_active(p);
        }
    }
}

// ---- creation (runs on TID_UI) ---------------------------------------------

fn open_on_ui() {
    // Creating a browser during shutdown races CefShutdown teardown and hangs.
    if jfn_shutting_down() {
        return;
    }
    if OPEN
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let kind = c"update";
    let layer = unsafe { jfn_browsers_create(kind.as_ptr()) };
    if layer.is_null() {
        OPEN.store(false, Ordering::Release);
        return;
    }

    let name = c"update";
    unsafe { jfn_cef_layer_set_name(layer, name.as_ptr()) };

    let l = unsafe { &*layer };
    let inner = unsafe { jfn_cef_layer_inner(layer) };
    *UPDATE_INNER.lock() = Some(Arc::clone(&inner));

    // setCreatedCallback — update dialog wins input whenever it's created.
    let inner_for_created = Arc::clone(&inner);
    l.set_created_callback_rust(Some(Box::new(move |_browser_raw: *mut c_void| {
        let p = inner_for_created.layer_ptr();
        if !p.is_null() {
            jfn_browsers_set_active(p);
        }
    })));

    // setMessageHandler — updateApply / updateLater.
    l.set_message_handler_rust(Some(Box::new(handle_message)));

    // setContextMenuBuilder / dispatcher — shared app menu.
    l.set_context_menu_builder_rust(Some(crate::app_menu::build_closure()));
    l.set_context_menu_dispatcher_rust(Some(crate::app_menu::dispatch_closure()));

    // BeforeClose: clear the singleton + drop the live references. The Browsers
    // registry removal + active-stack pop are unconditional in
    // `client::handle_on_before_close`.
    l.set_before_close_callback_rust(Some(Box::new(|| {
        OPEN.store(false, Ordering::Release);
        *UPDATE_INNER.lock() = None;
    })));

    unsafe {
        jfn_cef_layer_set_visible(layer, true);
        let url = "app://resources/update.html";
        jfn_cef_layer_create(layer, url.as_ptr() as *const _, url.len());
    }

    // Keep input ownership for the dialog's whole lifetime (see `start_heartbeat`).
    start_heartbeat();
}

fn handle_message(message: BrowserMessage) -> bool {
    match message.name() {
        "updateApply" => {
            // Spawn the installer (blocks on AppMutex until we exit), then
            // shut down cleanly so the locked CEF/mpv DLLs are released.
            fire_apply_hook();
            jfn_shutdown_initiate();
            true
        }
        "updateLater" => {
            if let Some(b) = message.browser()
                && let Some(host) = b.host()
            {
                host.close_browser(0);
            }
            true
        }
        _ => false,
    }
}
