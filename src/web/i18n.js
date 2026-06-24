// Lightweight i18n for the injected web UI (subtitle-style panel + Client
// Settings page). Injected right after native-shim.js, so window.jmpInfo
// already exists and window.jmpI18n is defined before any later script runs.
//
// Two jobs:
//   1. Expose window.jmpI18n.t(englishString) -> localized string (or the
//      English string unchanged when there's no entry / language is English).
//      Fork-owned scripts call this directly; see subtitle-style.js.
//   2. Translate window.jmpInfo.settingsDescriptions IN PLACE at load time, so
//      the upstream client-settings.js renders localized labels/help/options
//      without being edited (the bulk of the text lives there, in native-shim.js).
//
// Language follows the OS locale via navigator.language (wired to the system
// locale by the CEF accept_language_list fix), mirroring overlay.lang.js. To
// add a language, add a `<lang>` table below keyed by English source string.
// Anything missing from a table falls through to English (passthrough).
(function () {
    const FALLBACK = 'en';

    // English source string -> translation. Keyed by the exact English text so
    // it stays robust to the data layout: if upstream adds/renames a string the
    // key simply misses and English shows through — it never breaks.
    const ptBR = {
        // --- settingsDescriptions: displayName -------------------------------
        'Hardware Decoding': 'Decodificação por hardware',
        'Audio Passthrough': 'Passthrough de áudio',
        'Exclusive Audio Output': 'Saída de áudio exclusiva',
        'Audio Channel Layout': 'Layout de canais de áudio',
        'Subtitle Font': 'Fonte das legendas',
        'Force Transcoding': 'Forçar transcodificação',
        'Hide Scrollbar': 'Ocultar barra de rolagem',
        'Device Name': 'Nome do dispositivo',
        'Log Level': 'Nível de log',
        'Transparent Titlebar': 'Barra de título transparente',
        'Window Decorations': 'Decorações da janela',

        // --- settingsDescriptions: help --------------------------------------
        'Hardware video decoding mode. Use "auto" for automatic detection or "no" to disable.':
            'Modo de decodificação de vídeo por hardware. Use "auto" para detecção automática ou "no" para desativar.',
        'Comma-separated list of codecs to pass through to the audio device (e.g. ac3,eac3,dts-hd,truehd). Leave empty to disable.':
            'Lista de codecs separada por vírgulas para enviar diretamente ao dispositivo de áudio (ex.: ac3,eac3,dts-hd,truehd). Deixe vazio para desativar.',
        'Take exclusive control of the audio device during playback. May reduce latency but prevents other apps from playing audio.':
            'Assume controle exclusivo do dispositivo de áudio durante a reprodução. Pode reduzir a latência, mas impede que outros aplicativos reproduzam áudio.',
        'Force a specific channel layout. Leave empty for auto-detection.':
            'Força um layout de canais específico. Deixe vazio para detecção automática.',
        'Font family used for subtitles. Leave empty for the default. The font must be installed on this system. Size, color and outline are adjusted from the "Subtitle style" button in the player\'s Subtitles menu.':
            'Família de fonte usada nas legendas. Deixe vazio para a padrão. A fonte precisa estar instalada neste sistema. Tamanho, cor e contorno são ajustados pelo botão "Estilo das legendas" no menu de Legendas do player.',
        'Always request a transcoded stream from the server, even when direct play would work.':
            'Sempre solicita um stream transcodificado do servidor, mesmo quando a reprodução direta funcionaria.',
        'Hide scrollbars throughout the app. Scrolling with the wheel, trackpad, and keyboard still works. Requires restart.':
            'Oculta as barras de rolagem em todo o aplicativo. A rolagem pelo mouse, trackpad e teclado continua funcionando. Requer reinício.',
        'Identifies this machine to the server. Leave blank to use the system hostname.':
            'Identifica esta máquina para o servidor. Deixe em branco para usar o nome do host do sistema.',
        'Set the application log verbosity level.':
            'Define o nível de detalhamento do log do aplicativo.',
        'Overlay traffic light buttons on the window content instead of a separate titlebar. Requires restart.':
            'Sobrepõe os botões de controle ao conteúdo da janela em vez de uma barra de título separada. Requer reinício.',
        'How the window titlebar is drawn. In-app is needed on desktops without their own (e.g. GNOME). Auto-detected by default; changing requires restart.':
            'Como a barra de título da janela é desenhada. "No app" é necessário em desktops sem barra própria (ex.: GNOME). Detectado automaticamente por padrão; alterar requer reinício.',

        // --- settingsDescriptions: option titles -----------------------------
        'Auto': 'Automático',
        'Stereo': 'Estéreo',
        'Default (Info)': 'Padrão (Info)',
        'Verbose': 'Detalhado',
        'Debug': 'Depuração',
        'Warning': 'Aviso',
        'Error': 'Erro',
        'In-app (client-side)': 'No app (lado do cliente)',
        'System (server-side)': 'Sistema (lado do servidor)',
        'System, themed (KDE)': 'Sistema, com tema (KDE)',

        // --- Client Settings chrome (client-settings.js) ---------------------
        'Client Settings': 'Configurações do cliente',
        'Changes take effect after restarting the application.':
            'As alterações entram em vigor após reiniciar o aplicativo.',
        'Playback': 'Reprodução',
        'Audio': 'Áudio',
        'Subtitles': 'Legendas',
        'Transcode': 'Transcodificação',
        'Advanced': 'Avançado',
        'MPV config': 'Configuração do MPV',
        'Open mpv config directory': 'Abrir o diretório de configuração do mpv',
        'Server': 'Servidor',
        'Reset Saved Server': 'Redefinir servidor salvo',

        // --- Subtitle style panel (subtitle-style.js) ------------------------
        'White': 'Branco',
        'Yellow': 'Amarelo',
        'Black': 'Preto',
        'Gray': 'Cinza',
        'Off': 'Desligado',
        'Thin': 'Fino',
        'Medium': 'Médio',
        'Thick': 'Grosso',
        'Normal': 'Normal',
        'Bold': 'Negrito',
        'Subtitle styling': 'Estilo das legendas',
        'Subtitle style': 'Estilo das legendas',
        'Size': 'Tamanho',
        'Vertical position': 'Posição vertical',
        'Color': 'Cor',
        'Outline': 'Contorno',
        'Outline color': 'Cor do contorno',
        'Reset to defaults': 'Restaurar padrões',
        'Close': 'Fechar',
        'Smaller': 'Menor',
        'Larger': 'Maior',
        'Lower': 'Mais baixo',
        'Higher': 'Mais alto'
    };

    const TRANSLATIONS = {
        'pt-br': ptBR,
        'pt': ptBR
    };

    function detectLanguage() {
        let lang = (navigator.language || navigator.userLanguage
            || (navigator.languages && navigator.languages[0]) || FALLBACK).toLowerCase();
        if (TRANSLATIONS[lang]) return lang;
        const base = lang.split('-')[0];
        if (TRANSLATIONS[base]) return base;
        return FALLBACK;
    }

    const lang = detectLanguage();
    const table = TRANSLATIONS[lang] || null;

    // Translate an English source string; passthrough when no entry exists.
    function t(s) {
        if (!table || s == null) return s;
        const hit = table[s];
        return hit == null ? s : hit;
    }

    // Translate window.jmpInfo.settingsDescriptions in place. native-shim.js
    // (incl. its platform-specific unshift()s) ran synchronously before us, so
    // the object is fully built here. Raw string options (e.g. mpv decoder
    // names) have no `title` and are left untouched.
    function translateSettingsDescriptions() {
        const descs = window.jmpInfo && window.jmpInfo.settingsDescriptions;
        if (!descs) return;
        Object.keys(descs).forEach(function (section) {
            const list = descs[section];
            if (!Array.isArray(list)) return;
            list.forEach(function (setting) {
                if (!setting) return;
                if (setting.displayName) setting.displayName = t(setting.displayName);
                if (setting.help) setting.help = t(setting.help);
                if (Array.isArray(setting.options)) {
                    setting.options.forEach(function (opt) {
                        if (opt && typeof opt === 'object' && opt.title) {
                            opt.title = t(opt.title);
                        }
                    });
                }
            });
        });
    }

    try {
        if (table) translateSettingsDescriptions();
    } catch (e) {
        console.error('[i18n] settingsDescriptions translation failed:', e);
    }

    window.jmpI18n = { lang: lang, t: t };
})();
