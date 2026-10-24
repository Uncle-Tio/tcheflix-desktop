//! Tchê Flix: runtime translation of the native shell's English UI strings.
//!
//! Same contract and same tables as `web/i18n.js`: `web/i18n/<lang>.json`,
//! keyed by the exact English source string. A string upstream adds or rewords
//! has no entry and shows in English until the table catches up — it never
//! breaks. The language is the shell's own [`system_locale`].
//!
//! [`system_locale`]: crate::shell::lang::system_locale

use std::collections::HashMap;
use std::sync::OnceLock;

/// `(lowercase BCP 47 tag, table)`; matched exactly, then by primary subtag.
const TABLES: &[(&str, &str)] = &[("pt-br", include_str!("../../web/i18n/pt-BR.json"))];

/// `english` in the system language, or `english` itself when there is no
/// translation. Unit tests always see the English source, which upstream's
/// tests assert.
pub fn tr(english: &str) -> &str {
    if cfg!(test) {
        return english;
    }
    table()
        .and_then(|table| table.get(english))
        .map_or(english, String::as_str)
}

fn table() -> Option<&'static HashMap<String, String>> {
    static TABLE: OnceLock<Option<HashMap<String, String>>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let json = select(&crate::shell::lang::system_locale())?;
            serde_json::from_str(json).ok()
        })
        .as_ref()
}

fn select(locale: &str) -> Option<&'static str> {
    let primary = |tag: &str| tag.split('-').next().unwrap_or(tag).to_owned();
    TABLES
        .iter()
        .find(|(tag, _)| *tag == locale)
        .or_else(|| {
            TABLES
                .iter()
                .find(|(tag, _)| primary(tag) == primary(locale))
        })
        .map(|(_, json)| *json)
}

/// The OS UI language, lowercased with `_` folded to `-`, for platforms that
/// keep it outside the POSIX locale variables (Windows).
pub fn os_locale() -> Option<String> {
    sys_locale::get_locale().map(|locale| locale.to_lowercase().replace('_', "-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_is_a_flat_string_map() {
        for (tag, json) in TABLES {
            assert!(
                serde_json::from_str::<HashMap<String, String>>(json).is_ok(),
                "{tag}"
            );
        }
    }

    #[test]
    fn exact_tag_then_primary_subtag_then_none() {
        assert!(select("pt-br").is_some());
        assert_eq!(select("pt"), select("pt-br"));
        assert_eq!(select("pt-pt"), select("pt-br"));
        assert!(select("en-us").is_none());
    }
}
