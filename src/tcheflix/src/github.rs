//! GitHub Releases query for the Tchê Flix updater.
//!
//! Hits the public `releases/latest` endpoint of the fork repo. Any failure
//! (offline, rate-limit, malformed body, missing asset) maps to `None` so the
//! caller treats the app as up to date and starts normally. The endpoint is
//! overridable via the `TCHEFLIX_UPDATE_FEED` env var for local testing.

use serde::Deserialize;
use std::time::Duration;

const DEFAULT_FEED: &str =
    "https://api.github.com/repos/Uncle-Tio/tcheflix-desktop/releases/latest";

/// GitHub rejects requests without a User-Agent; identify ourselves.
const USER_AGENT: &str = concat!("tcheflix-updater/", env!("CARGO_PKG_VERSION"));

/// A release relevant to the updater: its tag, changelog body, and the
/// Windows setup asset to download.
#[derive(Debug, Clone)]
pub struct Release {
    pub tag: String,
    pub notes: String,
    pub asset_url: String,
    pub asset_name: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

fn feed_url() -> String {
    std::env::var("TCHEFLIX_UPDATE_FEED").unwrap_or_else(|_| DEFAULT_FEED.to_string())
}

pub(crate) fn is_setup_asset(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("tcheflixsetup") && n.ends_with(".exe")
}

/// Fetch the latest non-draft release, or `None` on any failure.
pub fn fetch_latest() -> Option<Release> {
    let url = feed_url();
    let body = http_get_string(&url)?;
    let api: ApiRelease = serde_json::from_str(&body).ok()?;
    if api.draft {
        return None;
    }
    let asset = api.assets.into_iter().find(|a| is_setup_asset(&a.name))?;
    Some(Release {
        tag: api.tag_name,
        notes: api.body,
        asset_url: asset.browser_download_url,
        asset_name: asset.name,
    })
}

fn http_get_string(url: &str) -> Option<String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .new_agent();
    let mut resp = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?;
    resp.body_mut().read_to_string().ok()
}
