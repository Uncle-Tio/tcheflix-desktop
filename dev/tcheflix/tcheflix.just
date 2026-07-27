# Tchê Flix fork-owned recipes (imported by the top-level justfile).
#
# Kept in a separate file so syncs with the upstream justfile rarely conflict.
# Windows-only: the fork ships only a Windows installer.

# Build the Windows installer (Inno Setup) → dist/TcheFlixSetup-<ver>.exe
[group('build')]
[windows]
installer: build
    & 'dev/windows/build_installer.ps1'
