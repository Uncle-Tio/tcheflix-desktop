; Tchê Flix — Windows installer (Inno Setup).
;
; One .exe, two modes (mode is chosen by who runs it, not the file):
;   * First install: user double-clicks the download → interactive wizard.
;   * Update: the app spawns this with `/SILENT /SUPPRESSMSGBOXES /NORESTART`
;     → progress bar only, no clicks; the [Run] entry relaunches afterwards.
;
; Per-user install (no UAC): PrivilegesRequired=lowest + {localappdata}. The
; AppId stays fixed so updates upgrade in place. Instead of the AppMutex
; directive (which aborts a silent run if the app is still alive), InitializeSetup
; WAITS for the app's named mutex to clear, so the locked CEF/mpv DLLs are
; released before we overwrite them.

#ifndef AppVer
  #define AppVer "0.0.0"
#endif
#define AppName "Tchê Flix"
#define AppExeName "jellyfin-desktop.exe"
#define AppMutexName "TcheFlixSingleInstance"
; Paths are resolved relative to this .iss (dev/windows).
#define RepoRoot "..\.."

[Setup]
AppId={{B7E4F2A1-9C3D-4E5F-8A6B-2D1C0F3E4A5B}
AppName={#AppName}
AppVersion={#AppVer}
AppPublisher=Uncle-Tio
AppPublisherURL=https://github.com/Uncle-Tio/tcheflix-desktop
VersionInfoVersion={#AppVer}
DefaultDirName={localappdata}\Programs\TcheFlix
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
RestartApplications=no
; Compression=lzma2/max
Compression=lzma2/normal
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#RepoRoot}\src\tcheflix\assets\tcheflix_icon.ico
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExeName}
OutputDir={#RepoRoot}\dist
OutputBaseFilename=TcheFlixSetup-{#AppVer}

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#RepoRoot}\build\install\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{userdesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
; No `skipifsilent`: this fires in BOTH modes — a "launch now" checkbox in the
; interactive install, and an automatic relaunch after a silent update.
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall

[Code]
function InitializeSetup(): Boolean;
var
  i: Integer;
begin
  // Wait up to ~30s for a running Tchê Flix to finish exiting (releasing the
  // locked CEF/mpv DLLs) before installing over it. No-op on a fresh install.
  i := 0;
  while CheckForMutexes('{#AppMutexName}') and (i < 120) do
  begin
    Sleep(250);
    i := i + 1;
  end;
  Result := True;
end;
