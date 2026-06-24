pub mod app;
mod cli;
mod instance_id;
pub mod manager;
mod platform_install;
#[cfg(target_os = "windows")]
mod updater;
mod window_geometry;
#[cfg(target_os = "linux")]
mod wl_interpose;
