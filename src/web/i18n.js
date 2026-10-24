// Tchê Flix: lightweight i18n for the fork's injected web UI (subtitle-style
// panel). Exposes window.jmpI18n.t(englishString) -> localized string, or the
// English string unchanged when there's no entry / the language is English.
//
// The tables are src/web/i18n/<lang>.json, keyed by the exact English source
// string — the same files the native shell translates through (jfn_rust's
// i18n.rs). embedded_js.rs prepends them at build time as
// window.__jmpI18nTables = {"<tag>": {...}}. A string missing from a table
// falls through to English, so a reworded string never breaks anything.
//
// Language follows the OS locale via navigator.language (wired to the system
// locale by the CEF accept_language_list fix): exact tag first, then any table
// with the same primary subtag.
(function () {
    const tables = window.__jmpI18nTables || {};
    delete window.__jmpI18nTables;

    function primary(tag) {
        return tag.split('-')[0];
    }

    function detectTable() {
        const lang = (navigator.language || '').toLowerCase();
        if (tables[lang]) return tables[lang];
        const tag = Object.keys(tables).find(function (t) { return primary(t) === primary(lang); });
        return tag ? tables[tag] : null;
    }

    const table = detectTable();

    // Translate an English source string; passthrough when no entry exists.
    function t(s) {
        if (!table || s == null) return s;
        const hit = table[s];
        return hit == null ? s : hit;
    }

    window.jmpI18n = { t: t };
})();
