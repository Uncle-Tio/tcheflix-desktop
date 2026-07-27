"""Launcher de teste do auto-updater do Tchê Flix.

Pede o link de um release (inclusive prerelease), resolve para o endpoint da API
do GitHub e abre o app com `TCHEFLIX_UPDATE_FEED` apontando para lá.

Aceita no campo de link: a tag pura (`v0.1.1`), a URL do release
(`.../releases/tag/v0.1.1`), a URL do asset (`.../releases/download/...`)
ou uma URL de API já pronta.
"""

from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

from rich.console import Console
from textual.app import App, ComposeResult
from textual.containers import Vertical
from textual.widgets import Button, Footer, Header, Input, Label, RadioButton, RadioSet, Static

REPO = "Uncle-Tio/tcheflix-desktop"
FEED_VAR = "TCHEFLIX_UPDATE_FEED"

# dev/tcheflix/<este arquivo> → dois níveis até a raiz do repo.
REPO_ROOT = Path(__file__).resolve().parents[2]
LOCALAPPDATA = Path(os.environ.get("LOCALAPPDATA", ""))

# Cada alvo lista candidatos em ordem de preferência: o rename para
# `tcheflix-desktop.exe` é recente, então installs antigos ainda têm o nome velho.
TARGETS: dict[str, list[Path]] = {
    "Executável Instalado": [
        LOCALAPPDATA / "Programs" / "TcheFlix" / "jellyfin-desktop.exe",
        LOCALAPPDATA / "Programs" / "TcheFlix" / "tcheflix-desktop.exe",
    ],
    "Build Dev": [
        REPO_ROOT / "build" / "jellyfin-desktop.exe",
        REPO_ROOT / "build" / "jellium-desktop.exe",
        REPO_ROOT / "build" / "tcheflix-desktop.exe",
    ],
}


def resolve_exe(target: str) -> Path | None:
    return next((p for p in TARGETS[target] if p.is_file()), None)


def resolve_feed(raw: str) -> str | None:
    """Normaliza o que o usuário colou para a URL de API `releases/tags/<tag>`.

    Esse endpoint devolve um objeto único no mesmo formato de `releases/latest`,
    mas — ao contrário dele — também enxerga prereleases.
    """
    text = raw.strip()
    if not text:
        return None
    if text.startswith("https://api.github.com/"):
        return text

    match = re.search(
        r"github\.com/(?P<repo>[^/]+/[^/]+)/releases/(?:tag|download)/(?P<tag>[^/?#]+)",
        text,
    )
    if match:
        return f"https://api.github.com/repos/{match['repo']}/releases/tags/{match['tag']}"

    if "/" not in text and " " not in text:  # tag pura
        return f"https://api.github.com/repos/{REPO}/releases/tags/{text}"
    return None


class LauncherApp(App):
    TITLE = "Tchê Flix — teste do auto-updater"
    CSS = """
    #form { padding: 1 2; }
    Label { margin-top: 1; }
    #status { margin-top: 1; height: 2; }
    .err { color: $error; }
    .ok { color: $success; }
    """
    BINDINGS = [("escape", "quit", "Sair")]

    def compose(self) -> ComposeResult:
        yield Header()
        with Vertical(id="form"):
            yield Label("Link do release (tag, URL do release ou do asset):")
            yield Input(
                placeholder=f"https://github.com/{REPO}/releases/tag/v0.1.1",
                id="link",
            )
            yield Label("Qual binário abrir?")
            with RadioSet(id="target"):
                for index, name in enumerate(TARGETS):
                    yield RadioButton(name, value=index == 0)
            yield Static(id="status")
            yield Button("Abrir com esse feed", variant="primary", id="go")
        yield Footer()

    def on_mount(self) -> None:
        self.query_one("#link", Input).focus()

    def on_input_submitted(self) -> None:
        self.launch()

    def on_button_pressed(self) -> None:
        self.launch()

    def fail(self, message: str) -> None:
        status = self.query_one("#status", Static)
        status.set_classes("err")
        status.update(message)

    def launch(self) -> None:
        feed = resolve_feed(self.query_one("#link", Input).value)
        if feed is None:
            self.fail("Não consegui extrair a tag desse link.")
            return

        pressed = self.query_one("#target", RadioSet).pressed_button
        if pressed is None:
            self.fail("Escolha qual binário abrir.")
            return

        target = str(pressed.label)
        exe = resolve_exe(target)
        if exe is None:
            candidates = "\n".join(f"  {p}" for p in TARGETS[target])
            self.fail(f"Nenhum exe encontrado para “{target}”:\n{candidates}")
            return

        self.exit({"feed": feed, "exe": exe, "target": target})


def main() -> None:
    console = Console()
    result = LauncherApp().run()
    if not result:
        console.print("[yellow]Cancelado.[/]")
        return

    exe: Path = result["exe"]
    feed: str = result["feed"]
    env = os.environ | {FEED_VAR: feed}

    console.print(f"[bold]Alvo:[/] {result['target']}\n[dim]{exe}[/]")
    console.print(f"[bold]{FEED_VAR}:[/]\n[dim]{feed}[/]")

    proc = subprocess.Popen(
        [str(exe)],
        env=env,
        cwd=exe.parent,
        creationflags=subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP,
    )
    console.print(f"[green]Aberto[/] (pid {proc.pid}).")

    for log in (REPO_ROOT / "build" / "run.log", LOCALAPPDATA / "tcheflix-desktop"):
        if log.exists():
            console.print(f"[dim]log: {log}[/]")


if __name__ == "__main__":
    main()
