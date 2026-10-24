pub mod app;
mod app_menu;
mod cli;
mod discord;
mod i18n;
pub mod manager;
mod platform_install;
pub mod shell;
#[cfg(target_os = "windows")]
mod updater;
mod window_geometry;

mod connection;
