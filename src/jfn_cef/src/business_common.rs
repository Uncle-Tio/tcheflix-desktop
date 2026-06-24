//! Shared helpers for the three `business_*` modules.
//!
//! Two groups, separated by the dividers below:
//!   1. Generic CEF/Rust helpers — could lift into a `cef-rs-helpers` crate.
//!   2. App-specific dispatch — Jellyfin-desktop config wiring.

use std::ffi::CString;

/// Returns true if the supplied `MutexGuard`-bearing `Option` already holds
/// a value — i.e. a singleton `init` is being called twice. Crashes loud in
/// debug; logs + returns true in release so a programmer error never
/// escalates. `caller` names the offending init for the log line.
pub(crate) fn reject_double_init<T>(slot: &Option<T>, caller: &str) -> bool {
    if slot.is_some() {
        debug_assert!(false, "{caller} called twice");
        jfn_logging::log(
            jfn_logging::CATEGORY_CEF,
            jfn_logging::LEVEL_WARN,
            &format!("{caller} called twice; ignoring"),
        );
        return true;
    }
    false
}

// --- generic Rust/C interop ------------------------------------------------

/// Convert a JS-supplied string into a `CString` for FFI, logging + dropping
/// on interior NUL. `label` names the IPC arm in the warn message so the
/// log line is enough to locate the offending handler.
///
/// Avoids the prior `CString::new(x).unwrap_or_default()` pattern that
/// silently handed `""` to downstream consumers (e.g. mpv).
pub(crate) fn js_cstr_or_warn(label: &str, s: &str) -> Option<CString> {
    match CString::new(s) {
        Ok(c) => Some(c),
        Err(_) => {
            jfn_logging::log(
                jfn_logging::CATEGORY_CEF,
                jfn_logging::LEVEL_WARN,
                &format!("{label}: interior NUL in JS string; dropping IPC"),
            );
            None
        }
    }
}

// --- app-specific dispatch -------------------------------------------------

/// Persist a string subtitle override and apply it to mpv. Empty means "no
/// override"; since mpv rejects an empty color/font, the built-in `default` is
/// sent live instead (persistence still stores "").
fn apply_sub_string(
    label: &str,
    value: &str,
    default: &str,
    persist: impl FnOnce(&str),
    apply: impl FnOnce(&std::ffi::CStr),
) {
    persist(value);
    let live = if value.is_empty() { default } else { value };
    if let Some(c) = js_cstr_or_warn(label, live) {
        apply(&c);
    }
}

/// `setSettingValue` IPC dispatch. Superset of the keys the overlay and the
/// main web UI send today — both UIs share this single source of truth so
/// new keys land in one place.
pub(crate) fn apply_setting_value(_section: &str, key: &str, value: &str) {
    match key {
        "hwdec" => jfn_config::set_hwdec(value),
        "audioPassthrough" => jfn_config::set_audio_passthrough(value),
        "audioExclusive" => jfn_config::set_audio_exclusive(value == "true"),
        "audioChannels" => jfn_config::set_audio_channels(value),
        "windowDecorations" => jfn_config::set_window_decorations(value),
        "hideScrollbar" => jfn_config::set_hide_scrollbar(value == "true"),
        "logLevel" => jfn_config::set_log_level(value),
        "forceTranscoding" => jfn_config::set_force_transcoding(value == "true"),
        // Pass empty platform_default — Rust setter clears when raw equals
        // the empty string. Neither caller has the live hostname handy here.
        "deviceName" => jfn_config::set_device_name(value, ""),
        // Subtitle styling: persist + apply live. The mpv setters no-op when
        // the handle isn't initialized, so calling here is always safe.
        "subScale" => {
            let v = value.parse::<f64>().unwrap_or(1.0);
            jfn_config::set_sub_scale(v);
            jfn_mpv::api::jfn_mpv_set_sub_scale(v);
        }
        "subFont" => apply_sub_string(
            key,
            value,
            "sans-serif",
            jfn_config::set_sub_font,
            jfn_mpv::api::jfn_mpv_set_sub_font,
        ),
        "subColor" => apply_sub_string(
            key,
            value,
            "#FFFFFFFF",
            jfn_config::set_sub_color,
            jfn_mpv::api::jfn_mpv_set_sub_color,
        ),
        "subBorderColor" => apply_sub_string(
            key,
            value,
            "#FF000000",
            jfn_config::set_sub_border_color,
            jfn_mpv::api::jfn_mpv_set_sub_border_color,
        ),
        "subBorderSize" => {
            let v = value.parse::<f64>().unwrap_or(3.0);
            jfn_config::set_sub_border_size(v);
            jfn_mpv::api::jfn_mpv_set_sub_border_size(v);
        }
        "subPos" => {
            let v = value.parse::<f64>().unwrap_or(100.0);
            jfn_config::set_sub_pos(v);
            jfn_mpv::api::jfn_mpv_set_sub_pos(v);
        }
        "subBold" => {
            let on = value == "true";
            jfn_config::set_sub_bold(on);
            jfn_mpv::api::jfn_mpv_set_sub_bold(on);
        }
        _ => jfn_logging::log(
            jfn_logging::CATEGORY_CEF,
            jfn_logging::LEVEL_WARN,
            &format!("Unknown setting key: {_section}.{key}"),
        ),
    }
    jfn_config::settings_save_async();
}
