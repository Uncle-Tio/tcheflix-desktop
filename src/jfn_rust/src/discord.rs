//! Tchê Flix: Discord Rich Presence wiring.
//!
//! Kept in its own file so upstream syncs don't conflict — app.rs only gains a
//! single `discord::start()` / `discord::stop()` pair. See `crate::updater` for
//! the same pattern.
//!
//! The on/off setting is read once here rather than inside the sink, which is
//! why the settings panel labels it as requiring a restart. The others are
//! handed over as `fn` pointers and read live on every presence update.

/// Base URL used only to build the cover-art link handed to Discord.
///
/// The explicit override wins when set: `server_url` is usually a LAN address,
/// and Discord's CDN fetches the poster from the public internet, so it can
/// never resolve one. Read live on every presence update rather than captured,
/// so editing the setting takes effect on the next item played.
///
/// A plain `fn` (not a closure) so `jfn-discord` keeps taking a function
/// pointer and stays free of a `jfn-config` dependency.
fn image_base() -> String {
    let custom = jfn_config::discord_image_base();
    if custom.trim().is_empty() {
        jfn_config::server_url()
    } else {
        custom
    }
}

/// Whether the Discord status line shows the item name instead of the app name.
/// Read live on every presence update, so this one needs no restart.
fn status_title() -> bool {
    jfn_config::discord_status_title()
}

pub(crate) fn start() {
    if !jfn_config::discord_rpc() {
        return;
    }
    jfn_discord::start(image_base, status_title);
}

pub(crate) fn stop() {
    jfn_discord::stop();
}
