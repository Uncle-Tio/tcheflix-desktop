//! Streaming download of the setup asset with progress reporting.
//!
//! Writes to a `.part` file; the caller renames atomically to the final name
//! only on full success, so a partial download is never mistaken for a ready
//! installer. Aborts promptly if shutdown is requested mid-flight.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

const USER_AGENT: &str = concat!("tcheflix-updater/", env!("CARGO_PKG_VERSION"));

fn to_io(e: ureq::Error) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// Download `url` to `dest`, invoking `on_progress` with 0..=100. `is_stopping`
/// is polled between chunks so a shutdown aborts the transfer.
pub fn download_to(
    url: &str,
    dest: &Path,
    mut on_progress: impl FnMut(u8),
    is_stopping: fn() -> bool,
) -> std::io::Result<()> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30 * 60)))
        .build()
        .new_agent();
    let mut resp = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(to_io)?;

    let total: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut reader = resp.body_mut().as_reader();
    let mut file = File::create(dest)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_pct: i32 = -1;

    loop {
        if is_stopping() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "shutdown requested",
            ));
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        // `checked_div` yields `None` while total is unknown (0), so progress
        // is simply not reported until a content-length is available.
        if let Some(pct) = downloaded
            .min(total)
            .checked_mul(100)
            .and_then(|scaled| scaled.checked_div(total))
        {
            let pct = pct.min(100) as u8;
            if i32::from(pct) != last_pct {
                last_pct = i32::from(pct);
                on_progress(pct);
            }
        }
    }
    file.flush()?;
    Ok(())
}
