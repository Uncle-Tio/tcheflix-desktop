// Tchê Flix update dialog. Rendered in a closed Shadow DOM for style/DOM
// isolation. `window._updateData = {version, notes, ready, progress}` is
// prepended to this file by the browser-side resource handler
// (src/jfn_cef/src/resource.rs) — do NOT depend on any IPC to arrive before
// first paint. Live progress/ready are pushed afterwards via exec_js, which
// calls window._tcheflixSetProgress / window._tcheflixSetReady.
(function () {
    var data = window._updateData || {};
    var version = data.version || '';
    var ready = !!data.ready;
    var progress = typeof data.progress === 'number' ? data.progress : -1;
    var applying = false;

    // ---- safe markdown -> HTML (escape first, then a small subset) ----------
    function esc(s) {
        return s.replace(/[&<>"]/g, function (c) {
            return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c];
        });
    }
    function inline(s) {
        // operates on already-escaped text
        s = s.replace(/`([^`]+)`/g, '<code>$1</code>');
        s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
        s = s.replace(/(^|[^*])\*([^*\n]+)\*/g, '$1<em>$2</em>');
        s = s.replace(/\b_([^_\n]+)_\b/g, '<em>$1</em>');
        s = s.replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
            '<a href="$2" target="_blank" rel="noreferrer noopener">$1</a>');
        // bare urls
        s = s.replace(/(^|[\s(])(https?:\/\/[^\s)]+)/g,
            '$1<a href="$2" target="_blank" rel="noreferrer noopener">$2</a>');
        return s;
    }
    function renderMarkdown(md) {
        if (!md) return '<p class="muted">Sem notas para esta versão.</p>';
        var lines = esc(md).replace(/\r\n/g, '\n').split('\n');
        var out = [];
        var i = 0;
        var listType = null; // 'ul' | 'ol'
        function closeList() { if (listType) { out.push('</' + listType + '>'); listType = null; } }
        var para = [];
        function flushPara() {
            if (para.length) { out.push('<p>' + inline(para.join('<br>')) + '</p>'); para = []; }
        }
        for (; i < lines.length; i++) {
            var line = lines[i];
            var trimmed = line.trim();
            // fenced code
            if (/^```/.test(trimmed)) {
                flushPara(); closeList();
                var buf = [];
                i++;
                for (; i < lines.length && !/^```/.test(lines[i].trim()); i++) buf.push(lines[i]);
                out.push('<pre><code>' + buf.join('\n') + '</code></pre>');
                continue;
            }
            if (trimmed === '') { flushPara(); closeList(); continue; }
            var h = /^(#{1,4})\s+(.*)$/.exec(trimmed);
            if (h) {
                flushPara(); closeList();
                var lvl = h[1].length;
                out.push('<h' + lvl + '>' + inline(h[2]) + '</h' + lvl + '>');
                continue;
            }
            if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) { flushPara(); closeList(); out.push('<hr>'); continue; }
            if (/^>\s?/.test(trimmed)) {
                flushPara(); closeList();
                out.push('<blockquote>' + inline(trimmed.replace(/^>\s?/, '')) + '</blockquote>');
                continue;
            }
            var ul = /^[-*+]\s+(.*)$/.exec(trimmed);
            var ol = /^\d+[.)]\s+(.*)$/.exec(trimmed);
            if (ul || ol) {
                flushPara();
                var want = ul ? 'ul' : 'ol';
                if (listType !== want) { closeList(); listType = want; out.push('<' + want + '>'); }
                out.push('<li>' + inline((ul || ol)[1]) + '</li>');
                continue;
            }
            para.push(trimmed);
        }
        flushPara(); closeList();
        return out.join('');
    }

    // ---- DOM ----------------------------------------------------------------
    var host = document.createElement('div');
    host.id = '_tcheflix_update';
    host.style.cssText = 'position:fixed;left:0;top:0;width:100vw;height:100vh;z-index:2147483647';
    var shadow = host.attachShadow({ mode: 'closed' });

    var style = document.createElement('style');
    style.textContent = [
        '*{margin:0;padding:0;box-sizing:border-box}',
        ':host{all:initial}',
        '.bg{position:fixed;inset:0;background:rgba(8,8,10,.66);backdrop-filter:blur(4px);' +
            'display:flex;align-items:center;justify-content:center}',
        '.card{width:540px;max-width:88vw;max-height:84vh;display:flex;flex-direction:column;' +
            'background:linear-gradient(180deg,#202024,#161618);border:1px solid #343438;' +
            'border-radius:14px;box-shadow:0 18px 56px rgba(0,0,0,.62);overflow:hidden;' +
            'font-family:-apple-system,Segoe UI,Roboto,sans-serif;color:#e6e6ea;' +
            'animation:pop .18s ease-out}',
        '@keyframes pop{from{opacity:0;transform:translateY(8px) scale(.985)}to{opacity:1;transform:none}}',
        '.head{padding:22px 24px 16px;display:flex;align-items:center;gap:13px}',
        '.spark{width:38px;height:38px;border-radius:10px;flex:0 0 auto;display:flex;' +
            'align-items:center;justify-content:center;color:#fff;' +
            'background:linear-gradient(135deg,#22c55e,#38bdf8);box-shadow:0 4px 16px rgba(34,170,200,.42)}',
        '.spark svg{width:21px;height:21px;display:block}',
        '.htxt{flex:1 1 auto;min-width:0}',
        '.title{font-size:16px;font-weight:600;letter-spacing:.2px}',
        '.sub{font-size:12px;color:#9a9aa2;margin-top:2px}',
        '.pill{flex:0 0 auto;display:flex;align-items:center;gap:7px;font-size:12px;font-weight:700;' +
            'background:rgba(255,255,255,.05);border:1px solid rgba(255,255,255,.10);' +
            'padding:5px 11px;border-radius:999px}',
        '.vcur{color:#ff6b6b}',
        '.varr{color:#7b7b83;font-weight:600}',
        '.vnew{color:#4ade80}',
        '.notes{flex:1 1 auto;overflow-y:auto;padding:4px 24px 8px;font-size:13.5px;line-height:1.62;color:#cfcfd6}',
        '.notes h1,.notes h2,.notes h3,.notes h4{margin:14px 0 6px;line-height:1.3;color:#f0f0f3}',
        '.notes h1{font-size:17px}.notes h2{font-size:15px}.notes h3{font-size:14px}.notes h4{font-size:13px}',
        '.notes p{margin:7px 0}',
        '.notes ul,.notes ol{margin:7px 0 7px 22px}.notes li{margin:3px 0}',
        '.notes a{color:#7fb0ff;text-decoration:none}.notes a:hover{text-decoration:underline}',
        '.notes code{font-family:ui-monospace,Consolas,monospace;font-size:12.5px;' +
            'background:#2a2a30;padding:1px 5px;border-radius:4px}',
        '.notes pre{background:#0f0f12;border:1px solid #2a2a30;border-radius:8px;' +
            'padding:10px 12px;margin:8px 0;overflow-x:auto}',
        '.notes pre code{background:none;padding:0}',
        '.notes blockquote{border-left:3px solid #3a3a42;padding:2px 0 2px 12px;margin:8px 0;color:#a6a6ad}',
        '.notes hr{border:none;border-top:1px solid #2a2a30;margin:12px 0}',
        '.muted{color:#8a8a92}',
        '.foot{flex:0 0 auto;padding:14px 24px 18px;border-top:1px solid #26262b;background:#141416}',
        '.prog{display:flex;align-items:center;gap:10px;margin-bottom:14px}',
        '.track{flex:1 1 auto;height:6px;border-radius:999px;background:#2a2a30;overflow:hidden;position:relative}',
        '.fill{height:100%;width:0;border-radius:999px;background:linear-gradient(90deg,#22c55e,#38bdf8);' +
            'transition:width .25s ease}',
        '.fill.indet{width:34% !important;animation:slide 1.1s ease-in-out infinite}',
        '@keyframes slide{0%{margin-left:-34%}100%{margin-left:100%}}',
        '.pstat{font-size:12px;color:#9a9aa2;flex:0 0 auto;min-width:96px;text-align:right}',
        '.row{display:flex;justify-content:space-between;align-items:center;gap:10px}',
        '.btn{font:inherit;font-size:13px;font-weight:600;padding:9px 18px;border-radius:9px;' +
            'cursor:pointer;border:1px solid transparent;transition:background .14s,opacity .14s}',
        '.ghost{background:transparent;border-color:#3a3a42;color:#c4c4cc}',
        '.ghost:hover{background:#212127}',
        '.primary{background:linear-gradient(135deg,#22c55e,#0ea5e9);color:#fff;box-shadow:0 3px 12px rgba(34,170,200,.4)}',
        '.primary:hover{background:linear-gradient(135deg,#2bd169,#22a7f0)}',
        '.btn:disabled{opacity:.45;cursor:default;box-shadow:none}'
    ].join('');
    shadow.appendChild(style);

    var bg = document.createElement('div'); bg.className = 'bg';
    var card = document.createElement('div'); card.className = 'card';
    bg.appendChild(card);
    shadow.appendChild(bg);

    card.innerHTML =
        '<div class="head">' +
            '<div class="spark">' +
                '<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">' +
                    '<path d="M19 9h-4V3H9v6H5l7 7 7-7zM5 18v2h14v-2H5z"/></svg>' +
            '</div>' +
            '<div class="htxt"><div class="title">Atualização disponível</div>' +
                '<div class="sub">Uma nova versão do Tchê Flix está pronta.</div></div>' +
            '<div class="pill" id="ver"></div>' +
        '</div>' +
        '<div class="notes" id="notes"></div>' +
        '<div class="foot">' +
            '<div class="prog"><div class="track"><div class="fill" id="fill"></div></div>' +
                '<div class="pstat" id="pstat"></div></div>' +
            '<div class="row">' +
                '<button class="btn ghost" id="later">Mais tarde</button>' +
                '<button class="btn primary" id="apply" disabled>Atualizar agora</button>' +
            '</div>' +
        '</div>';

    var verEl = card.querySelector('#ver');
    if (version) {
        var current = (data.current || '').toString().trim();
        verEl.innerHTML =
            (current ? '<span class="vcur">v' + esc(current) + '</span><span class="varr">→</span>' : '') +
            '<span class="vnew">v' + esc(version) + '</span>';
    } else {
        verEl.style.display = 'none';
    }
    card.querySelector('#notes').innerHTML = renderMarkdown(data.notes || '');

    var fill = card.querySelector('#fill');
    var pstat = card.querySelector('#pstat');
    var applyBtn = card.querySelector('#apply');
    var laterBtn = card.querySelector('#later');

    function render() {
        if (applying) {
            fill.classList.remove('indet'); fill.style.width = '100%';
            pstat.textContent = 'Fechando…';
            applyBtn.disabled = true; laterBtn.disabled = true;
            applyBtn.textContent = 'Instalando…';
            return;
        }
        if (ready) {
            fill.classList.remove('indet'); fill.style.width = '100%';
            pstat.textContent = 'Pronto para instalar';
            applyBtn.disabled = false;
        } else if (progress >= 0) {
            fill.classList.remove('indet'); fill.style.width = progress + '%';
            pstat.textContent = 'Baixando ' + progress + '%';
            applyBtn.disabled = true;
        } else {
            fill.classList.add('indet');
            pstat.textContent = 'Preparando…';
            applyBtn.disabled = true;
        }
    }

    var done = false;
    function later() {
        if (done) return; done = true;
        window.removeEventListener('keydown', onKey, true);
        host.remove();
        if (window.jmpNative && window.jmpNative.updateLater) window.jmpNative.updateLater();
    }
    function apply() {
        if (done || !ready || applying) return;
        applying = true; render();
        if (window.jmpNative && window.jmpNative.updateApply) window.jmpNative.updateApply();
    }
    function onKey(e) {
        if (e.key === 'Escape') { e.preventDefault(); later(); }
        else if (e.key === 'Enter' && ready && !applying) { e.preventDefault(); apply(); }
    }

    laterBtn.addEventListener('click', later);
    applyBtn.addEventListener('click', apply);
    window.addEventListener('keydown', onKey, true);

    // live updates pushed from Rust
    window._tcheflixSetProgress = function (p) {
        progress = p | 0; if (!ready) render();
    };
    window._tcheflixSetReady = function () { ready = true; render(); };

    render();
    document.body.appendChild(host);
})();
