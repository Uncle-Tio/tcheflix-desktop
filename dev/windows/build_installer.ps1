# Build the Tchê Flix Windows installer (Inno Setup).
#
# Assumes `just build` already staged build/. Stages a clean install tree into
# build/install (exe + CEF + mpv only), then compiles dev/windows/installer.iss
# with ISCC. The installer version is the `tcheflix` crate's own version
# (independent of the upstream workspace version).

$ErrorActionPreference = "Stop"
$RepoRoot = (Get-Item $PSScriptRoot).Parent.Parent.FullName

# Tchê Flix version = first `version = "..."` in src/tcheflix/Cargo.toml ([package]).
$manifest = Join-Path $RepoRoot "src\tcheflix\Cargo.toml"
$verMatch = Select-String -Path $manifest -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $verMatch) { throw "could not read tcheflix version from $manifest" }
$ver = $verMatch.Matches[0].Groups[1].Value
Write-Host "Tchê Flix version: $ver" -ForegroundColor Cyan

Push-Location $RepoRoot
try {
    # Clean prefix so removed files don't linger between builds.
    $prefix = Join-Path $RepoRoot "build\install"
    if (Test-Path $prefix) { Remove-Item -Recurse -Force $prefix }

    # Stage exe + CEF + mpv into build/install (file copy only; no recompile).
    $mpv = Join-Path $RepoRoot "third_party\mpv-install"
    & cargo xtask install --prefix build/install --skip-build --external-mpv $mpv
    if ($LASTEXITCODE -ne 0) { throw "cargo xtask install failed" }

    # Compile the installer → dist/TcheFlixSetup-<ver>.exe.
    $iscc = "C:\Programas\Scoop\apps\innosetup-np\current\ISCC.exe"
    if (-not (Test-Path $iscc)) { throw "ISCC.exe not found at $iscc" }
    & $iscc "/DAppVer=$ver" (Join-Path $RepoRoot "dev\windows\installer.iss")
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }

    Write-Host "Installer written: dist\TcheFlixSetup-$ver.exe" -ForegroundColor Green
}
finally {
    Pop-Location
}
