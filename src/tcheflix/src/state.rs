//! Persisted updater state: last-check timestamp and a skipped version.
//!
//! Stored as JSON in `%LOCALAPPDATA%\jellyfin-desktop\updates\state.json`
//! (reusing `jfn_paths::cache_dir`). All operations are best-effort: a missing
//! or corrupt file yields defaults, and write failures are ignored.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub last_check: u64,
    #[serde(default)]
    pub skip_version: Option<String>,
}

fn state_path() -> PathBuf {
    crate::updates_dir().join("state.json")
}

pub fn load() -> State {
    std::fs::read(state_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(state: &State) {
    if let Ok(bytes) = serde_json::to_vec_pretty(state) {
        let _ = jfn_paths::write_atomic(&state_path(), &bytes);
    }
}
