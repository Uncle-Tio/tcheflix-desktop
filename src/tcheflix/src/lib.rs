//! Tchê Flix Windows auto-updater (fork-owned, self-contained).
//!
//! Runs a background check against the GitHub Releases of the fork repo on
//! startup; if a newer version exists it downloads the setup asset and reports
//! progress through a caller-supplied callback. The CEF dialog and the
//! app-shutdown handoff live in the main app — this crate only owns the
//! networking, versioning, and the detached installer spawn.
//!
//! Decoupling note: every failure path is silent (`UpToDate`) so the app always
//! boots, and the crate never depends on `jfn-cef`/`jfn-playback` (it takes the
//! shutdown predicate as a plain `fn`), keeping it a leaf with no cycles.

mod download;
mod github;
mod state;
pub mod version;

mod apply;
pub use apply::apply;
pub use version::TCHEFLIX_VERSION;

use std::path::{Path, PathBuf};

/// Events emitted by the background updater, in order, to the callback passed
/// to [`start`]. A run ends in exactly one of `Ready`, `UpToDate`, or `Error`.
#[derive(Debug, Clone)]
pub enum UpdateEvent {
    /// A newer release exists; the dialog should open with this changelog.
    Available { version: String, notes: String },
    /// Download progress, 0..=100.
    Progress(u8),
    /// The installer finished downloading and is ready to apply.
    Ready(PathBuf),
    /// No update (or check failed) — nothing to show.
    UpToDate,
    /// An update existed but downloading it failed.
    Error,
}

pub(crate) fn updates_dir() -> PathBuf {
    let dir = jfn_paths::cache_dir().join("updates");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Remove accumulated installers plus any orphaned `.part` downloads, keeping
/// only the setup named `keep` — or nothing when `keep` is `None` (the app is
/// already on the latest version).
fn cleanup_stale(keep: Option<&str>) {
    let Ok(entries) = std::fs::read_dir(updates_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let is_part = name.ends_with(".part");
        let is_stale_setup = github::is_setup_asset(name) && keep != Some(name);
        if is_part || is_stale_setup {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Spawn the background update check. Returns immediately; `cb` is invoked on
/// the updater thread. `is_stopping` is polled to abort promptly on shutdown.
pub fn start<F>(is_stopping: fn() -> bool, cb: F)
where
    F: Fn(UpdateEvent) + Send + 'static,
{
    let spawned = std::thread::Builder::new()
        .name("tcheflix-updater".to_string())
        .spawn(move || run(is_stopping, &cb));
    if spawned.is_err() {
        tracing::warn!(target: "tcheflix", "failed to spawn updater thread");
    }
}

fn run(is_stopping: fn() -> bool, cb: &dyn Fn(UpdateEvent)) {
    if is_stopping() {
        return;
    }

    let Some(release) = github::fetch_latest() else {
        tracing::debug!(target: "tcheflix", "no release found / check skipped");
        cb(UpdateEvent::UpToDate);
        return;
    };

    // Keep the cached installer only when the latest release is a genuine
    // upgrade we'll offer; if it matches the version we're already running, drop
    // it too so an up-to-date install leaves nothing behind. Orphaned `.part`
    // files always go.
    let is_newer = version::is_newer(&release.tag);
    cleanup_stale(is_newer.then_some(release.asset_name.as_str()));

    let mut st = state::load();
    st.last_check = now_unix();
    state::save(&st);

    if !is_newer {
        cb(UpdateEvent::UpToDate);
        return;
    }
    if st.skip_version.as_deref() == Some(release.tag.as_str()) {
        tracing::info!(target: "tcheflix", "release {} skipped by user", release.tag);
        cb(UpdateEvent::UpToDate);
        return;
    }

    let version = version::parse_tag(&release.tag)
        .map(|v| v.to_string())
        .unwrap_or_else(|| release.tag.clone());
    tracing::info!(target: "tcheflix", "update available: {version}");
    cb(UpdateEvent::Available {
        version,
        notes: release.notes.clone(),
    });

    download_and_report(&release, is_stopping, cb);
}

fn download_and_report(
    release: &github::Release,
    is_stopping: fn() -> bool,
    cb: &dyn Fn(UpdateEvent),
) {
    let dir = updates_dir();
    let dest = dir.join(&release.asset_name);

    // A complete file from a previous session: the atomic rename guarantees
    // only fully-downloaded installers ever land at `dest`.
    if dest.is_file() {
        cb(UpdateEvent::Ready(dest));
        return;
    }

    let part = dir.join(format!("{}.part", release.asset_name));
    let _ = std::fs::remove_file(&part);

    let result = download::download_to(
        &release.asset_url,
        &part,
        |pct| cb(UpdateEvent::Progress(pct)),
        is_stopping,
    );

    match result {
        Ok(()) if rename_into_place(&part, &dest) => {
            cb(UpdateEvent::Ready(dest));
        }
        _ => {
            let _ = std::fs::remove_file(&part);
            if is_stopping() {
                return;
            }
            tracing::warn!(target: "tcheflix", "update download failed");
            cb(UpdateEvent::Error);
        }
    }
}

fn rename_into_place(part: &Path, dest: &Path) -> bool {
    std::fs::rename(part, dest).is_ok()
}

/// Persist a "skip this version" choice (so the dialog stops nagging for it).
pub fn skip_version(tag: &str) {
    let mut st = state::load();
    st.skip_version = Some(tag.to_string());
    state::save(&st);
}
