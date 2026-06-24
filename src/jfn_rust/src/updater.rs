//! Windows-only wiring for the Tchê Flix auto-updater (fork-owned glue).
//!
//! Bridges the leaf `tcheflix` crate (networking/versioning/installer spawn)
//! to the `jfn_cef` update dialog. Kept in its own file so upstream syncs don't
//! conflict — app.rs only gains a single guarded `updater::start()` call.
//!
//! Flow: `tcheflix::start` runs a background check; its events drive the CEF
//! dialog. The dialog's "Atualizar" button fires the apply-hook installed here,
//! which spawns the downloaded installer. We also hold a named mutex matching
//! the installer's `AppMutex` so the silent upgrade waits until we fully exit
//! (releasing the locked CEF/mpv DLLs).

use std::path::PathBuf;

use parking_lot::Mutex;
use tcheflix::UpdateEvent;

/// Must match `AppMutex` in dev/windows/installer.iss.
const APP_MUTEX_NAME: &str = "TcheFlixSingleInstance";

/// Path to the downloaded installer once `Ready`, consumed by the apply-hook.
static READY_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Start the background update check and wire the dialog's "Atualizar" button.
/// Call once, after the main browser is up.
pub fn start() {
    hold_app_mutex();
    jfn_cef::business_update::jfn_update_set_apply_hook(Box::new(apply));
    tcheflix::start(jfn_playback::shutdown::jfn_shutting_down, on_event);
}

fn on_event(ev: UpdateEvent) {
    match ev {
        UpdateEvent::Available { version, notes } => {
            jfn_cef::business_update::jfn_update_open(
                tcheflix::TCHEFLIX_VERSION.to_string(),
                version,
                notes,
            );
        }
        UpdateEvent::Progress(pct) => {
            jfn_cef::business_update::jfn_update_push_progress(pct);
        }
        UpdateEvent::Ready(path) => {
            *READY_PATH.lock() = Some(path);
            jfn_cef::business_update::jfn_update_set_ready();
        }
        UpdateEvent::UpToDate => {
            tracing::debug!(target: "tcheflix", "up to date");
        }
        UpdateEvent::Error => {
            tracing::warn!(target: "tcheflix", "update download failed");
        }
    }
}

/// Fired when the user clicks "Atualizar": launch the downloaded installer.
/// `business_update` calls `jfn_shutdown_initiate()` right after, so the app
/// exits and the installer (waiting on the app mutex) takes over.
fn apply() {
    // Clone out of the lock first — don't hold it across the process spawn.
    let ready = READY_PATH.lock().clone();
    match ready {
        Some(path) => {
            if let Err(e) = tcheflix::apply(&path) {
                tracing::error!(target: "tcheflix", "failed to launch installer: {e}");
            }
        }
        None => tracing::warn!(target: "tcheflix", "apply requested but no installer ready"),
    }
}

/// Create the named mutex and leak the handle: it stays open for the process
/// lifetime so Inno Setup's `AppMutex` check blocks the silent upgrade until we
/// exit. The OS closes the handle (and destroys the mutex) when we terminate.
fn hold_app_mutex() {
    // Minimal kernel32 FFI keeps this self-contained (no extra crate/feature).
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(
            attrs: *const core::ffi::c_void,
            initial_owner: i32,
            name: *const u16,
        ) -> *mut core::ffi::c_void;
    }

    let name: Vec<u16> = APP_MUTEX_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `name` is a valid NUL-terminated UTF-16 buffer; null attrs is
    // allowed. The returned handle is intentionally leaked (never closed).
    let handle = unsafe { CreateMutexW(core::ptr::null(), 0, name.as_ptr()) };
    let _ = handle;
}
