@echo off
rem Mirror of the repository-root jackett_launcher.bat for the Rust binaries.
rem
rem Fixes https://github.com/Jackett/Jackett/issues/10068: when the update is
rem performed by a background service there is no interactive desktop, so the
rem updater hands off to this script. It waits until jackett-updater exits,
rem then starts the tray (or the console server when the tray is unavailable).

setlocal EnableExtensions
cd /d "%~dp0"

if /i "%JACKETT_PROFILE%"=="release" (
  set "BIN=%CD%\target\release"
) else (
  set "BIN=%CD%\target\debug"
)

:loop
tasklist /FI "IMAGENAME eq jackett-updater.exe" /NH 2>nul | find /i "jackett-updater.exe" >nul
if not errorlevel 1 (
  echo jackett-updater is still running, waiting...
  timeout /t 1 /nobreak >nul
  goto loop
)

if exist "%BIN%\jackett-tray.exe" (
  start "" "%BIN%\jackett-tray.exe" --UpdatedVersion yes
) else (
  start "" "%BIN%\jackett-server.exe"
)
exit /b 0
