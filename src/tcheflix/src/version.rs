//! Independent Tchê Flix versioning + semver comparison.
//!
//! [`TCHEFLIX_VERSION`] is the version baked into the running build (the
//! `version` of this crate's `Cargo.toml`), deliberately decoupled from the
//! upstream workspace version. The updater compares it against the release tag
//! reported by GitHub (`v<major.minor.patch>`).

/// Version baked into this build. Bump `src/tcheflix/Cargo.toml` to release.
pub const TCHEFLIX_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Parse a release tag (`v1.2.3` or `1.2.3`) into a semver version.
pub fn parse_tag(tag: &str) -> Option<semver::Version> {
    let trimmed = tag.trim().trim_start_matches('v');
    semver::Version::parse(trimmed).ok()
}

/// The running build's version as semver, if parseable.
pub fn current() -> Option<semver::Version> {
    semver::Version::parse(TCHEFLIX_VERSION).ok()
}

/// True iff `tag` names a version strictly newer than the running build.
pub fn is_newer(tag: &str) -> bool {
    match (parse_tag(tag), current()) {
        (Some(remote), Some(local)) => remote > local,
        _ => false,
    }
}
