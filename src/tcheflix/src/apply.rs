//! Hand off to the downloaded Inno Setup installer.
//!
//! Spawns the setup fully detached (own process group, no console) with silent
//! flags so it shows only a progress bar and no wizard pages, then returns
//! immediately — the caller is responsible for shutting the app down so the
//! installer can replace the locked CEF/mpv DLLs (it waits on the app's
//! `AppMutex`). The `Child` is dropped without `wait()` so it survives our exit.

use std::path::Path;

#[cfg(target_os = "windows")]
pub fn apply(installer: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    Command::new(installer)
        .args([
            "/SILENT",
            "/SUPPRESSMSGBOXES",
            "/CLOSEAPPLICATIONS",
            "/NORESTART",
        ])
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn apply(_installer: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the Tchê Flix installer is Windows-only",
    ))
}
