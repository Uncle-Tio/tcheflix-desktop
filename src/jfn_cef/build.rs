use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("CARGO_MANIFEST_DIR has no grandparent")?;

    // Tchê Flix: the outward-facing version is the FORK's independent version
    // (src/tcheflix/Cargo.toml), not the upstream workspace one. It flows into
    // both JFN_APP_VERSION and JFN_APP_VERSION_FULL below, so the appVersion the
    // Jellyfin server sees, the HTTP user-agent, and the About/logs all report
    // the fork version. Falls back to the workspace version if the read fails so
    // the build never breaks. `env!` (not std::env::var) records the workspace
    // Cargo.toml as a dep so this re-runs when that version bumps too.
    println!("cargo:rerun-if-changed=../Cargo.toml");
    println!("cargo:rerun-if-changed=../tcheflix/Cargo.toml");
    let version =
        read_tcheflix_version(repo_root).unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=JFN_APP_VERSION={version}");

    // VERSION_FULL = "<VERSION>+<git short hash>[-dirty]", but only for
    // pre-release VERSIONs (those with a "-suffix"); a clean release stays
    // bare. xtask injects JFN_GIT_HASH/JFN_GIT_DIRTY as the authoritative
    // source; fall back to gitoxide for a bare `cargo build`.
    println!("cargo:rerun-if-env-changed=JFN_GIT_HASH");
    println!("cargo:rerun-if-env-changed=JFN_GIT_DIRTY");
    println!("cargo:rerun-if-env-changed=CEF_RESOURCES_DIR");
    let (git_hash, dirty) = match std::env::var("JFN_GIT_HASH") {
        Ok(h) if !h.is_empty() => {
            let dirty = std::env::var("JFN_GIT_DIRTY").as_deref() == Ok("1");
            (h, dirty)
        }
        _ => git_info(repo_root),
    };
    let version_full = if !version.contains('-') || git_hash.is_empty() {
        version.to_string()
    } else if dirty {
        format!("{version}+{git_hash}-dirty")
    } else {
        format!("{version}+{git_hash}")
    };
    println!("cargo:rustc-env=JFN_APP_VERSION_FULL={version_full}");
    track_git_refs(repo_root);

    let web_dir = repo_root.join("src").join("web");
    for entry in std::fs::read_dir(&web_dir)?.flatten() {
        let p = entry.path();
        println!("cargo:rerun-if-changed={}", p.display());
    }
    Ok(())
}

/// Fallback for bare `cargo build` (no xtask). Empty hash when there is no repo.
fn git_info(repo_root: &std::path::Path) -> (String, bool) {
    let Ok(repo) = gix::discover(repo_root) else {
        return (String::new(), false);
    };
    let hash = repo
        .head_id()
        .ok()
        .map(|id| id.to_hex_with_len(7).to_string())
        .unwrap_or_default();
    let dirty = repo.is_dirty().unwrap_or(false);
    (hash, dirty)
}

/// Re-run when HEAD moves. git_dir holds HEAD; common_dir holds refs/packed-refs
/// (they differ under a linked worktree).
fn track_git_refs(repo_root: &std::path::Path) {
    let Ok(repo) = gix::discover(repo_root) else {
        return;
    };
    println!(
        "cargo:rerun-if-changed={}",
        repo.git_dir().join("HEAD").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repo.common_dir().join("packed-refs").display()
    );
    if let Ok(Some(r)) = repo.head_ref() {
        let name = r.name().as_bstr().to_string();
        println!(
            "cargo:rerun-if-changed={}",
            repo.common_dir().join(name).display()
        );
    }
}

/// Read the fork's independent version from `src/tcheflix/Cargo.toml`'s
/// `[package]` `version = "x.y.z"`. A minimal line scan (no `toml` dep needed):
/// the package version is the first top-level `version = "..."` in the file,
/// well before any dependency's inline `version = "..."`. Returns `None` on any
/// failure so the caller falls back to the workspace version.
fn read_tcheflix_version(repo_root: &std::path::Path) -> Option<String> {
    let manifest = repo_root.join("src").join("tcheflix").join("Cargo.toml");
    let text = std::fs::read_to_string(manifest).ok()?;
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("version") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let start = rest.find('"')? + 1;
        let end = rest[start..].find('"')? + start;
        return Some(rest[start..end].to_string());
    }
    None
}
