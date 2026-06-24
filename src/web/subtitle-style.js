// In-player subtitle styling panel.
//
// Adds a "Subtitle style" button to jellyfin-web's "Subtitles" track menu; it
// opens a floating panel to adjust size/color/outline (the font stays in Client
// Settings). Every change goes through setValue('subtitles', ...) — the same
// path Client Settings uses — so it applies live in mpv and persists.
//
// jellyfin-web isn't vendored here, so the menu hook keys off the OSD subtitle
// button class (.btnSubtitles); see the click handler. Wrapped in try/catch
// since this script is concatenated ahead of csd.js / select-menu.js.
(function () {
    const HOST_TAG = 'jmp-subtitle-style';

    // Localize an English UI string via the i18n shim (injected before us);
    // passthrough to English if it isn't present for any reason.
    const t = function (s) { return window.jmpI18n ? window.jmpI18n.t(s) : s; };

    // Subtitle size stepper bounds (the sub-scale multiplier).
    const SCALE_MIN = 0.5;
    const SCALE_MAX = 3.0;
    const SCALE_STEP = 0.1;
    // App defaults (mirror SettingsData::default in config/lib.rs): 150%
    // scale + thin (1.5) outline, mimicking the old QT Jellyfin Media Player.
    const SCALE_DEFAULT = 1.5;
    const BORDER_SIZE_DEFAULT = 1.5;

    // Stored as mpv sub-pos (0=top, 100=bottom); the UI works in offset space
    // (0 = default, + raises the subtitles).
    const POS_DEFAULT = 100;
    const POS_OFFSET_MIN = -10;
    const POS_OFFSET_MAX = 50;
    const POS_OFFSET_STEP = 5;

    const COLOR_OPTIONS = [
        { value: '#FFFFFFFF', title: t('White') },
        { value: '#FFFFFF00', title: t('Yellow') },
        { value: '#FF000000', title: t('Black') },
        { value: '#FF808080', title: t('Gray') }
    ];
    const BORDER_SIZE_OPTIONS = [
        { value: '0', title: t('Off') },
        { value: '1.5', title: t('Thin') },
        { value: '3', title: t('Medium') },
        { value: '5', title: t('Thick') }
    ];
    const BOLD_OPTIONS = [
        { value: 'false', title: t('Normal') },
        { value: 'true', title: t('Bold') }
    ];

    let open = false;

    // The subtitle and audio menus are identical actionSheets, so we tell them
    // apart by which OSD button opened: set on a .btnSubtitles click, the next
    // actionSheet is the subtitle one. Cleared on a short timeout so a stale
    // flag never leaks into a later (e.g. audio) sheet.
    let expectSubSheet = false;
    let expectTimer = 0;

    let host = null;
    let panel = null;
    let colorEl = null;
    let borderSizeEl = null;
    let borderColorEl = null;
    let boldEl = null;
    let steppers = []; // readout updaters run by refresh()

    const CSS = `
        :host {
            all: initial;
            position: fixed; inset: 0;
            z-index: 2147483646;
            pointer-events: none;
            font: 13px/1.3 system-ui, sans-serif;
        }
        /* Pinned panel; click-through everywhere except the panel itself. */
        .panel {
            position: absolute; top: 64px; right: 16px;
            width: 296px;
            display: none; flex-direction: column; gap: 15px;
            pointer-events: auto;
            background: rgba(20, 20, 22, 0.96); color: #eee;
            border: 1px solid rgba(255, 255, 255, 0.12);
            border-radius: 12px; padding: 18px;
            box-shadow: 0 6px 24px rgba(0, 0, 0, 0.55);
        }
        :host([data-open="1"]) .panel { display: flex; }

        .header { display: flex; align-items: center; gap: 4px; }
        .header .title { flex: 1; font-size: 16px; font-weight: 600; }
        .close, .reset {
            all: unset; width: 30px; height: 30px; border-radius: 6px;
            display: flex; align-items: center; justify-content: center;
            cursor: pointer; color: #ccc; font-size: 19px;
        }
        .close:hover, .reset:hover { background: rgba(255, 255, 255, 0.15); color: #fff; }

        .row { display: flex; flex-direction: column; gap: 8px; }
        .label {
            font-size: 14px; text-transform: uppercase;
            letter-spacing: 0.04em; opacity: 0.65;
        }

        .stepper { display: flex; align-items: center; gap: 10px; }
        .stepper button {
            all: unset; width: 38px; height: 38px; border-radius: 7px;
            display: flex; align-items: center; justify-content: center;
            background: rgba(255, 255, 255, 0.1); cursor: pointer;
            font-size: 22px; color: #fff;
        }
        .stepper button:hover { background: rgba(255, 255, 255, 0.2); }
        .stepper .val {
            flex: 1; text-align: center; font-size: 18px;
            font-variant-numeric: tabular-nums;
        }

        .swatches { display: flex; gap: 10px; flex-wrap: wrap; }
        .swatch {
            width: 32px; height: 32px; border-radius: 50%;
            cursor: pointer; box-sizing: border-box;
            border: 2px solid transparent; padding: 1px;
        }
        .swatch[data-sel="1"] { border-color: #4ea1ff; }
        .swatch .chip {
            width: 100%; height: 100%; border-radius: 50%;
            border: 1px solid rgba(255, 255, 255, 0.3); box-sizing: border-box;
        }
        .seg { display: flex; gap: 8px; }
        .seg button {
            all: unset; flex: 1; text-align: center; padding: 8px 0;
            border-radius: 7px; background: rgba(255, 255, 255, 0.1);
            cursor: pointer; font-size: 15px; color: #fff;
        }
        .seg button:hover { background: rgba(255, 255, 255, 0.18); }
        .seg button[data-sel="1"] { background: #2f6db5; }`;

    function subtitleSettings() {
        try { return window.jmpInfo.settings.subtitles; } catch (e) { return null; }
    }

    function curValue(key) {
        const s = subtitleSettings();
        return s ? s[key] : undefined;
    }

    function curNumber(key, dflt) {
        const v = parseFloat(curValue(key));
        return isNaN(v) ? dflt : v;
    }

    // mpv colors are #AARRGGBB; the swatch ignores alpha (presets are opaque).
    function mpvColorToCss(v) {
        if (!v) return null;
        if (v.charAt(0) === '#' && v.length === 9) return '#' + v.slice(3);
        return v;
    }

    // Apply a setting: live in mpv + persisted (native side), and mirror the
    // value locally so reopening the panel shows the current choice.
    function setSub(key, value) {
        const v = String(value);
        if (window.api && window.api.settings
            && typeof window.api.settings.setValue === 'function') {
            window.api.settings.setValue('subtitles', key, v);
        }
        const s = subtitleSettings();
        if (s) {
            if (key === 'subScale' || key === 'subBorderSize' || key === 'subPos') {
                s[key] = parseFloat(v);
            } else if (key === 'subBold') {
                s[key] = (v === 'true');
            } else {
                s[key] = v;
            }
        }
    }

    function round1(x) { return Math.round(x * 10) / 10; }

    function buildSwatchRow(key, opts) {
        const wrap = document.createElement('div');
        wrap.className = 'swatches';
        opts.forEach(function (opt) {
            const sw = document.createElement('div');
            sw.className = 'swatch';
            sw.title = opt.title;
            sw.dataset.value = opt.value;
            const chip = document.createElement('div');
            chip.className = 'chip';
            const css = mpvColorToCss(opt.value);
            if (css) chip.style.background = css;
            sw.appendChild(chip);
            sw.addEventListener('click', function () {
                setSub(key, opt.value);
                refresh();
            });
            wrap.appendChild(sw);
        });
        return wrap;
    }

    function buildSeg(key, opts) {
        const wrap = document.createElement('div');
        wrap.className = 'seg';
        opts.forEach(function (opt) {
            const b = document.createElement('button');
            b.textContent = opt.title;
            b.dataset.value = opt.value;
            b.addEventListener('click', function () {
                setSub(key, opt.value);
                refresh();
            });
            wrap.appendChild(b);
        });
        return wrap;
    }

    // Stepper bound to a value via get/set adapters. The value may be a derived
    // unit (e.g. a position offset), so set() does any conversion back to the
    // stored key. Registers its readout in `steppers` for refresh() to sync.
    function buildStepper(cfg) {
        const wrap = document.createElement('div');
        wrap.className = 'stepper';
        const minus = document.createElement('button');
        minus.textContent = '−';
        minus.setAttribute('aria-label', cfg.decLabel);
        const val = document.createElement('div');
        val.className = 'val';
        const plus = document.createElement('button');
        plus.textContent = '+';
        plus.setAttribute('aria-label', cfg.incLabel);
        function step(d) {
            cfg.set(Math.min(cfg.max, Math.max(cfg.min, round1(cfg.get() + d))));
            refresh();
        }
        minus.addEventListener('click', function () { step(-cfg.step); });
        plus.addEventListener('click', function () { step(cfg.step); });
        wrap.append(minus, val, plus);
        steppers.push(function () { val.textContent = cfg.fmt(cfg.get()); });
        return wrap;
    }

    function makeRow(labelText) {
        const row = document.createElement('div');
        row.className = 'row';
        const lbl = document.createElement('div');
        lbl.className = 'label';
        lbl.textContent = labelText;
        row.appendChild(lbl);
        return row;
    }

    // Mark the child whose data-value matches (string-compared, so numeric and
    // string presets share one path).
    function select(container, value) {
        if (!container) return;
        const want = String(value);
        Array.prototype.forEach.call(container.children, function (c) {
            c.dataset.sel = (String(c.dataset.value) === want) ? '1' : '0';
        });
    }

    // Sync the panel controls to the current saved values.
    function refresh() {
        if (!panel) return;
        steppers.forEach(function (f) { f(); });
        const color = curValue('subColor');
        select(colorEl, color == null ? '' : color);
        select(borderSizeEl, curNumber('subBorderSize', BORDER_SIZE_DEFAULT));
        const borderColor = curValue('subBorderColor');
        select(borderColorEl, borderColor == null ? '' : borderColor);
        select(boldEl, curValue('subBold') ? 'true' : 'false');
    }

    function buildPanel() {
        panel.textContent = '';
        steppers = [];

        // Header — title + reset + close (X).
        const header = document.createElement('div');
        header.className = 'header';
        const title = document.createElement('div');
        title.className = 'title';
        title.textContent = t('Subtitle styling');
        // Reset only this panel's controls; the font lives in Client Settings.
        const resetBtn = document.createElement('button');
        resetBtn.className = 'reset';
        resetBtn.setAttribute('aria-label', t('Reset to defaults'));
        resetBtn.title = t('Reset to defaults');
        resetBtn.textContent = '↺';
        resetBtn.addEventListener('click', function () {
            setSub('subScale', SCALE_DEFAULT);
            setSub('subPos', POS_DEFAULT);
            setSub('subBold', 'false');
            setSub('subColor', '');
            setSub('subBorderSize', BORDER_SIZE_DEFAULT);
            setSub('subBorderColor', '');
            refresh();
        });
        const closeBtn = document.createElement('button');
        closeBtn.className = 'close';
        closeBtn.setAttribute('aria-label', t('Close'));
        closeBtn.title = t('Close');
        closeBtn.textContent = '✕';
        closeBtn.addEventListener('click', function () { closePanel(); });
        header.append(title, resetBtn, closeBtn);
        panel.appendChild(header);

        // Size — stepper (percentage of the sub-scale multiplier).
        const sizeRow = makeRow(t('Size'));
        sizeRow.appendChild(buildStepper({
            get: function () { return curNumber('subScale', SCALE_DEFAULT); },
            set: function (v) { setSub('subScale', v); },
            min: SCALE_MIN, max: SCALE_MAX, step: SCALE_STEP,
            fmt: function (v) { return Math.round(v * 100) + '%'; },
            decLabel: t('Smaller'), incLabel: t('Larger')
        }));
        panel.appendChild(sizeRow);

        // Vertical position — stepper in offset space (+ raises the subtitles).
        const posRow = makeRow(t('Vertical position'));
        posRow.appendChild(buildStepper({
            get: function () { return POS_DEFAULT - curNumber('subPos', POS_DEFAULT); },
            set: function (off) { setSub('subPos', POS_DEFAULT - off); },
            min: POS_OFFSET_MIN, max: POS_OFFSET_MAX, step: POS_OFFSET_STEP,
            fmt: function (off) { return off > 0 ? '+' + off : String(off); },
            decLabel: t('Lower'), incLabel: t('Higher')
        }));
        panel.appendChild(posRow);

        const boldRow = makeRow(t('Bold'));
        boldEl = buildSeg('subBold', BOLD_OPTIONS);
        boldRow.appendChild(boldEl);
        panel.appendChild(boldRow);

        const colorRow = makeRow(t('Color'));
        colorEl = buildSwatchRow('subColor', COLOR_OPTIONS);
        colorRow.appendChild(colorEl);
        panel.appendChild(colorRow);

        const outlineRow = makeRow(t('Outline'));
        borderSizeEl = buildSeg('subBorderSize', BORDER_SIZE_OPTIONS);
        outlineRow.appendChild(borderSizeEl);
        panel.appendChild(outlineRow);

        // Outline color — same swatches as the text color.
        const outlineColorRow = makeRow(t('Outline color'));
        borderColorEl = buildSwatchRow('subBorderColor', COLOR_OPTIONS);
        outlineColorRow.appendChild(borderColorEl);
        panel.appendChild(outlineColorRow);

        refresh();
    }

    function update() {
        if (host) host.setAttribute('data-open', open ? '1' : '0');
    }

    function openPanel() { open = true; refresh(); update(); }
    function closePanel() { open = false; update(); }

    function build() {
        if (host) return;
        host = document.createElement(HOST_TAG);
        const root = host.attachShadow({ mode: 'open' });

        const style = document.createElement('style');
        style.textContent = CSS;
        root.appendChild(style);

        panel = document.createElement('div');
        panel.className = 'panel';
        buildPanel();
        root.appendChild(panel);

        // init() runs at/after DOMContentLoaded, so documentElement exists.
        document.documentElement.appendChild(host);

        // Esc closes the panel (swallowed so it doesn't also exit fullscreen)
        // only while the panel is open.
        window.addEventListener('keydown', function (e) {
            if (e.key === 'Escape' && open) {
                closePanel();
                e.preventDefault();
                e.stopPropagation();
            }
        }, true);
    }

    // --- jellyfin "Subtitles" menu hook -----------------------------------

    // Last-resort cleanup if jellyfin's own close didn't fire: remove the dialog
    // container, every backdrop and the sheet, and restore page scroll — never
    // leave the user with a dimmed, click-blocking overlay.
    function manualCleanup(sheet) {
        if (sheet && sheet.remove) sheet.remove();
        const kill = document.querySelectorAll('.dialogContainer, .dialogBackdrop');
        for (let i = 0; i < kill.length; i++) kill[i].remove();
        document.body.style.removeProperty('overflow');
        document.documentElement.style.removeProperty('overflow');
    }

    // Fully dismiss the subtitle actionSheet, then resolve so the panel opens
    // only after the dim/focus is gone (like jellyfin's "Playback Info").
    //
    // jellyfin's actionSheet is a history-backed modal (`div.dialog` in
    // `div.dialogContainer`, dimmed by `div.dialogBackdrop`) that ignores
    // synthetic Escape / backdrop clicks; only `history.back()` tears down the
    // container + backdrop + focus trap. `manualCleanup` is the safety net.
    function dismissSheet(sheet) {
        return new Promise(function (resolve) {
            try { window.history.back(); } catch (e) { /* ignore */ }
            let frames = 0;
            (function waitClosed() {
                const gone = (!sheet || !sheet.isConnected)
                    && !document.querySelector('.dialogContainer')
                    && !document.querySelector('.dialogBackdrop');
                if (gone) { resolve(); return; }
                if (frames++ > 40) { manualCleanup(sheet); resolve(); return; }
                requestAnimationFrame(waitClosed);
            })();
        });
    }

    // Inject the "Subtitle style" pill into a subtitle actionSheet — but only
    // the sheet opened by the OSD subtitle button (see expectSubSheet). Returns
    // true once handled (or already injected / not the subtitle sheet).
    function injectIntoSheet(sheet) {
        if (sheet.__jmpSubInjected) return true;
        if (!expectSubSheet) return false;
        expectSubSheet = false;
        sheet.__jmpSubInjected = true;

        const icon = document.createElement('button');
        icon.type = 'button';
        icon.setAttribute('aria-label', t('Subtitle style'));
        icon.title = t('Subtitle style');
        // Material icon + label. jellyfin-web always loads the Material Icons
        // font (its whole OSD uses it); if it ever fails, the label still reads.
        const glyph = document.createElement('span');
        glyph.className = 'material-icons';
        glyph.textContent = 'text_format';
        glyph.style.cssText = 'font-size:18px;line-height:1';
        const label = document.createElement('span');
        label.textContent = t('Subtitle style');
        icon.append(glyph, label);
        // Inline styles: this element lives in jellyfin's DOM, not our shadow
        // root, so our scoped CSS doesn't reach it.
        icon.style.cssText = [
            'position:absolute', 'top:10px', 'right:12px',
            'height:32px', 'padding:0 10px', 'gap:6px',
            'border:0', 'border-radius:6px', 'cursor:pointer',
            'background:rgba(255,255,255,0.12)', 'color:#fff',
            'font:600 13px/1 system-ui,sans-serif', 'z-index:1',
            'display:inline-flex', 'align-items:center', 'justify-content:center'
        ].join(';');
        icon.addEventListener('mouseenter', function () {
            icon.style.background = 'rgba(255,255,255,0.24)';
        });
        icon.addEventListener('mouseleave', function () {
            icon.style.background = 'rgba(255,255,255,0.12)';
        });
        icon.addEventListener('click', function (e) {
            e.preventDefault();
            e.stopPropagation();
            // Close the menu (releasing its dim/focus) before showing the panel.
            dismissSheet(sheet).then(function () { openPanel(); });
        });

        // Anchor to the title's container so absolute positioning lands in the
        // sheet's top-right corner.
        const title = sheet.querySelector('.actionSheetTitle');
        const anchor = (title && title.parentElement) || sheet;
        if (window.getComputedStyle(anchor).position === 'static') {
            anchor.style.position = 'relative';
        }
        anchor.appendChild(icon);
        return true;
    }

    function handleSheet(sheet) {
        if (injectIntoSheet(sheet)) return;
        requestAnimationFrame(function () { injectIntoSheet(sheet); });
    }

    function startMenuObserver() {
        // Flag the next actionSheet as the subtitle one when the OSD subtitle
        // button is clicked (capture phase — the icon is a child <span>).
        document.addEventListener('click', function (e) {
            if (e.target.closest && e.target.closest('.btnSubtitles')) {
                expectSubSheet = true;
                clearTimeout(expectTimer);
                expectTimer = setTimeout(function () { expectSubSheet = false; }, 2000);
            }
        }, true);

        const observer = new MutationObserver(function (mutations) {
            for (let i = 0; i < mutations.length; i++) {
                const added = mutations[i].addedNodes;
                for (let j = 0; j < added.length; j++) {
                    const n = added[j];
                    if (n.nodeType !== 1) continue;
                    if (n.classList && n.classList.contains('actionSheet')) {
                        handleSheet(n);
                    } else if (n.querySelector) {
                        const sheets = n.querySelectorAll('.actionSheet');
                        for (let k = 0; k < sheets.length; k++) handleSheet(sheets[k]);
                    }
                }
            }
        });
        observer.observe(document.body, { childList: true, subtree: true });
    }

    function init() {
        try {
            build();
            startMenuObserver();
            window.__jmpSubtitleStyle = {
                open: openPanel,
                close: closePanel,
                toggle: function () { open ? closePanel() : openPanel(); }
            };
        } catch (e) {
            console.error('[subtitle-style] init failed:', e);
        }
    }

    // Injected during V8 context creation, before the DOM exists — appending
    // the host or observing document.body then would throw. Defer to ready.
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }
})();
