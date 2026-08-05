//! Discord Rich Presence integration.
//!
//! Fork-owned (Tchê Flix). Upstream has no equivalent, so this crate is kept
//! self-contained and depends only on `jfn-playback`: the server URL arrives as
//! a plain `fn` rather than a `jfn-config` dependency, keeping this a leaf with
//! no cycles (same decoupling rule as `src/jfn_rust/src/updater.rs`).

mod projection;
mod sink;

/// Start the presence sink.
///
/// Must run after `jfn_playback::ffi::jfn_playback_init()` — the coordinator
/// silently drops sink registrations made before it exists.
///
/// `server_url` and `status_title` are read live on every update rather than
/// captured once, so the poster URL follows the user switching servers
/// mid-session and the status-line setting takes effect without a restart.
pub fn start(server_url: fn() -> String, status_title: fn() -> bool) {
    sink::start(server_url, status_title);
}

/// Stop the presence sink and clear the activity. Safe to call when not started.
pub fn stop() {
    sink::stop();
}
