@echo off
rem Build the Jackett desktop (Tauri) app in release mode.
rem
rem Output: target\release\jackett-desktop.exe
rem
rem The desktop UI lives in jackett-desktop\wwwroot\index.html and is compiled
rem into the binary with include_str!, so there is no separate frontend build
rem step — a plain cargo build of the jackett-desktop package is enough.
rem
rem Any running instance is stopped first: a running binary is locked by
rem Windows, which makes cargo fail with "os error 5" while overwriting it.
rem The process appears under either name depending on the Tauri host, so both
rem variants are killed.

setlocal EnableExtensions
cd /d "%~dp0"

where cargo >nul 2>&1
if errorlevel 1 (
  echo cargo not found on PATH. Install Rust from https://rustup.rs/
  exit /b 1
)

rem Stop a running desktop instance (ignore "not found" errors).
taskkill /IM jackett-desktop.exe /F >nul 2>&1
taskkill /IM jackett_desktop.exe /F >nul 2>&1

echo Building jackett-desktop ^(release^)...
cargo build --release -p jackett-desktop
if errorlevel 1 (
  echo Desktop release build failed.
  exit /b 1
)

echo.
echo Done: target\release\jackett-desktop.exe
exit /b 0
