//! Tchê Flix auto-updater dialog business logic.
//!
//! The dialog is `web/update.js`, injected into jellyfin-web (the process's one
//! browser) and drawn in a closed Shadow DOM above the page. This module owns
//! the dialog's state and pushes it into the page through an exec_js sink the
//! main app installs; the page pulls the same state with `updateQuery` when it
//! loads, so an update found before jellyfin-web is up (or across a reload) is
//! still shown. "Atualizar" fires a Rust apply-hook (installed by the main app)
//! before initiating a clean shutdown so the installer can take over.
//!
//! Decoupling: this is a fork-owned file. The only shared-file touch points
//! are the three native functions + the injected script (injection.rs,
//! embedded_js.rs), the `pub mod` line (lib.rs), and one delegating line in
//! `business_web::handle_message`.

use parking_lot::Mutex;
use std::sync::OnceLock;

use crate::ipc::BrowserMessage;
use jfn_playback::shutdown::jfn_shutdown_initiate;

/// What the dialog renders. `None` until the updater offers a release.
static STATE: Mutex<Option<UpdateState>> = Mutex::new(None);

/// Installed by the main app: spawns the downloaded installer. Fired on
/// "Atualizar" just before shutdown.
static APPLY_HOOK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Installed by the main app: runs JS in jellyfin-web's main frame. Callable
/// from any thread; a no-op while no page is live.
static EXEC_JS: OnceLock<fn(&str)> = OnceLock::new();

#[derive(Clone, serde::Serialize)]
struct UpdateState {
    /// The running version.
    current: String,
    /// The newer release being offered.
    version: String,
    /// Its raw markdown changelog.
    notes: String,
    ready: bool,
    /// Download progress 0..=100, or -1 before the first tick.
    progress: i32,
    /// The user chose "Mais tarde": a page (re)load must not reopen it.
    #[serde(skip)]
    dismissed: bool,
}

/// Install the apply-hook (called once at startup by the main app).
pub fn jfn_update_set_apply_hook(f: Box<dyn Fn() + Send + Sync>) {
    let _ = APPLY_HOOK.set(f);
}

/// Install the exec_js sink (called once at startup by the main app).
pub fn jfn_update_set_exec_js(f: fn(&str)) {
    let _ = EXEC_JS.set(f);
}

fn exec_js(js: &str) {
    if let Some(f) = EXEC_JS.get() {
        f(js);
    }
}

// ---- public entry points (callable from any thread) ------------------------

/// Open the update dialog. `current` is the running version, `version` the
/// newer release being offered, and `notes` its raw markdown changelog.
pub fn jfn_update_open(current: String, version: String, notes: String) {
    *STATE.lock() = Some(UpdateState {
        current,
        version,
        notes,
        ready: false,
        progress: -1,
        dismissed: false,
    });
    push_open();
}

/// Update the download progress bar (0..=100).
pub fn jfn_update_push_progress(pct: u8) {
    if let Some(state) = STATE.lock().as_mut() {
        state.progress = i32::from(pct);
    }
    exec_js(&format!(
        "window._tcheflixSetProgress&&window._tcheflixSetProgress({pct});"
    ));
}

/// Enable the "Atualizar" button — the installer finished downloading.
pub fn jfn_update_set_ready() {
    if let Some(state) = STATE.lock().as_mut() {
        state.ready = true;
    }
    exec_js("window._tcheflixSetReady&&window._tcheflixSetReady();");
}

/// Push the whole dialog state into the page, unless there is none to show.
fn push_open() {
    let json = match STATE.lock().as_ref() {
        Some(state) if !state.dismissed => jfn_js_json::to_js_json(state),
        _ => None,
    };
    if let Some(json) = json {
        exec_js(&format!(
            "window._tcheflixUpdateOpen&&window._tcheflixUpdateOpen({json});"
        ));
    }
}

/// jellyfin-web's update IPC names. `None` for any other name, so the caller
/// keeps routing it.
pub(crate) fn handle_message(message: &BrowserMessage) -> Option<bool> {
    match message.name() {
        "updateQuery" => push_open(),
        "updateApply" => {
            // Spawn the installer (blocks on AppMutex until we exit), then
            // shut down cleanly so the locked CEF/mpv DLLs are released.
            if let Some(f) = APPLY_HOOK.get() {
                f();
            }
            jfn_shutdown_initiate();
        }
        "updateLater" => {
            if let Some(state) = STATE.lock().as_mut() {
                state.dismissed = true;
            }
        }
        _ => return None,
    }
    Some(true)
}
