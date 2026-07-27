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
; The build tree keeps upstream's binary name; we install it under ours instead
; of renaming [[bin]], which would mean diverging across xtask and the platform
; recipes. Nothing depends on the file name — CEF relaunches itself through
; std::env::current_exe(), and single-instance is keyed by its own string.
#define BuiltExeName "jellium-desktop.exe"
#define AppExeName "tcheflix-desktop.exe"
; Shipped before the fork owned these names; cleaned up on upgrade.
#define LegacyExeName "jellyfin-desktop.exe"
#define LegacyDataDirName "jellyfin-desktop"
#define DataDirName "tcheflix-desktop"
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

[InstallDelete]
; Left over from installs made before the exe was renamed — the new one lands
; beside it otherwise, and the old shortcuts keep pointing at it.
Type: files; Name: "{app}\{#LegacyExeName}"

[Files]
; The executable is the one file that gets renamed on the way in; everything
; else (CEF, mpv, resources) copies verbatim.
Source: "{#RepoRoot}\build\install\{#BuiltExeName}"; DestDir: "{app}"; DestName: "{#AppExeName}"; Flags: ignoreversion
Source: "{#RepoRoot}\build\install\*"; DestDir: "{app}"; Excludes: "{#BuiltExeName}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{userdesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
; No `skipifsilent`: this fires in BOTH modes — a "launch now" checkbox in the
; interactive install, and an automatic relaunch after a silent update.
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall

[Code]
// Move a pre-fork data directory to the name jfn_paths uses now. One-time by
// nature — drop this whole section once every install has been through it.

// Entry by entry, never one rename of the whole directory: that is all-or-nothing,
// and a single file still held open under it vetoes the move.
//
// `updates` cannot come along — the running setup was downloaded into it — so it
// stays behind and CleanupLegacyData sweeps it on the next version.
function MoveContents(Legacy, Current: String): Boolean;
var
  FindRec: TFindRec;
  Entries: TStringList;
  i, Failed: Integer;
begin
  Entries := TStringList.Create;
  try
    // Collect first, move after: mutating a directory while enumerating it is
    // asking for trouble.
    if FindFirst(AddBackslash(Legacy) + '*', FindRec) then
    begin
      try
        repeat
          if (FindRec.Name <> '.') and (FindRec.Name <> '..') and
             (CompareText(FindRec.Name, 'updates') <> 0) then
            Entries.Add(FindRec.Name);
        until not FindNext(FindRec);
      finally
        FindClose(FindRec);
      end;
    end;

    Failed := 0;
    for i := 0 to Entries.Count - 1 do
    begin
      if not RenameFile(AddBackslash(Legacy) + Entries[i],
                        AddBackslash(Current) + Entries[i]) then
      begin
        Failed := Failed + 1;
        Log('Could not move ' + Entries[i] + ' out of ' + Legacy);
      end;
    end;
    Result := Failed = 0;
  finally
    Entries.Free;
  end;
end;

// The legacy directory is still there next to the current one, so the migration
// already happened and what is left is leftovers — in practice the `updates`
// folder it could not take.
procedure CleanupLegacyData(Legacy: String);
begin
  if DelTree(Legacy, True, True, True) then
    Log('Cleaned up legacy data: ' + Legacy)
  else
    Log('Could not clean up legacy data: ' + Legacy);
end;

procedure MigrateDataDir(Base: String);
var
  Legacy, Current: String;
begin
  Legacy := AddBackslash(Base) + '{#LegacyDataDirName}';
  Current := AddBackslash(Base) + '{#DataDirName}';
  if not DirExists(Legacy) then
    Exit;

  if DirExists(Current) then
  begin
    CleanupLegacyData(Legacy);
    Exit;
  end;

  if not ForceDirectories(Current) then
  begin
    Log('Could not create data dir: ' + Current);
    Exit;
  end;

  if MoveContents(Legacy, Current) then
  begin
    Log('Migrated data dir: ' + Legacy + ' -> ' + Current);
    RemoveDir(Legacy);  // no-op while `updates` is still in there
  end
  else
    Log('Migrated data dir with errors: ' + Legacy + ' -> ' + Current);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  // ssInstall: after the mutex wait, before any file is written.
  if CurStep = ssInstall then
  begin
    // %APPDATA% holds settings.json + the mpv config; %LOCALAPPDATA% holds the
    // logs, the updater state and the CEF profile (i.e. the login session).
    MigrateDataDir(ExpandConstant('{userappdata}'));
    MigrateDataDir(ExpandConstant('{localappdata}'));
  end;
end;

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
