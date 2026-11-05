@echo off
rem Jackett (Rust) batch launcher.
rem Usage: run.bat [action] [extra arguments passed to the binary]
rem
rem Actions:
rem   server (default)  build if needed and run jackett-server in foreground
rem   service           run jackett-service (add --service for the SCM host)
rem   install           install the Windows service  (administrator required)
rem   uninstall         uninstall the Windows service (administrator required)
rem   start / stop / status   manage the installed service
rem   tray              start jackett-tray
rem   updater           run jackett-updater (pass e.g. --check or --path ...)
rem   check             check GitHub for the latest release
rem   launcher          run the post-update launcher
rem   build / test      cargo build / test the whole workspace
rem   help              show this text
rem
rem Set JACKETT_PROFILE=release to use target\release instead of target\debug.

setlocal EnableExtensions DisableDelayedExpansion
cd /d "%~dp0"

if /i "%JACKETT_PROFILE%"=="release" (
  set "BIN=%CD%\target\release"
  set "PROFILE=--release"
) else (
  set "BIN=%CD%\target\debug"
  set "PROFILE="
)

set "ACTION=%~1"
if "%ACTION%"=="" set "ACTION=server"

set "ARGS="
:collect
shift
if "%~1"=="" goto collected
set "ARGS=%ARGS% %1"
goto collect
:collected

if /i "%ACTION%"=="help"     goto help
if /i "%ACTION%"=="-h"       goto help
if /i "%ACTION%"=="--help"   goto help
if /i "%ACTION%"=="build"    goto build
if /i "%ACTION%"=="test"     goto test
if /i "%ACTION%"=="server"   goto server
if /i "%ACTION%"=="run"      goto server
if /i "%ACTION%"=="service"  goto service
if /i "%ACTION%"=="install"  goto install
if /i "%ACTION%"=="uninstall" goto uninstall
if /i "%ACTION%"=="start"    goto start
if /i "%ACTION%"=="stop"     goto stop
if /i "%ACTION%"=="status"   goto status
if /i "%ACTION%"=="tray"     goto tray
if /i "%ACTION%"=="updater"  goto updater
if /i "%ACTION%"=="update"   goto updater
if /i "%ACTION%"=="check"    goto check
if /i "%ACTION%"=="launcher" goto launcher
echo Unknown action: %ACTION%
echo.
goto help

:help
echo Usage: run.bat [action] [extra arguments]
echo.
echo   server ^(default^)  run jackett-server in foreground
echo   service          run jackett-service ^(use --service for the SCM host^)
echo   install          install the Windows service ^(admin^)
echo   uninstall        uninstall the Windows service ^(admin^)
echo   start / stop / status   manage the installed service
echo   tray             start jackett-tray
echo   updater          run jackett-updater ^(e.g. --path C:\Jackett^)
echo   check            check GitHub for the latest release
echo   launcher         post-update launcher
echo   build / test     cargo build / test --workspace
echo   help             show this text
echo.
echo Examples:
echo   run.bat
echo   run.bat server --port 9118
echo   run.bat install
echo   run.bat updater --update-to v0.22.100 --path C:\Jackett
echo   set JACKETT_PROFILE=release ^& run.bat server
exit /b 0

:ensure
rem When running from a source checkout, always do an incremental build so the
rem batch launcher never starts a stale binary after the sources changed.
if not exist "Cargo.toml" goto ensure_check
where cargo >nul 2>&1
if errorlevel 1 goto ensure_check
cargo build --workspace %PROFILE% > "%TEMP%\jackett-build.log" 2>&1
if errorlevel 1 (
  echo Build failed:
  type "%TEMP%\jackett-build.log"
  exit /b 1
)
:ensure_check
if exist "%BIN%\%~1" exit /b 0
echo %~1 not found. Run "run.bat build" first.
exit /b 1

:require_admin
net session >nul 2>&1
if errorlevel 1 (
  echo This action requires administrator privileges.
  echo Right-click run.bat and choose "Run as administrator".
  exit /b 1
)
exit /b 0

:build
cargo build --workspace %PROFILE% %ARGS%
exit /b %errorlevel%

:test
cargo test --workspace %PROFILE% %ARGS%
exit /b %errorlevel%

:server
call :ensure jackett-server.exe || exit /b 1
echo Starting jackett-server ^(Ctrl+C to stop^)...
"%BIN%\jackett-server.exe" %ARGS%
exit /b %errorlevel%

:service
call :ensure jackett-service.exe || exit /b 1
"%BIN%\jackett-service.exe" %ARGS%
exit /b %errorlevel%

:install
call :require_admin || exit /b 1
call :ensure jackett-service.exe || exit /b 1
"%BIN%\jackett-service.exe" --install %ARGS%
exit /b %errorlevel%

:uninstall
call :require_admin || exit /b 1
call :ensure jackett-service.exe || exit /b 1
"%BIN%\jackett-service.exe" --uninstall %ARGS%
exit /b %errorlevel%

:start
call :require_admin || exit /b 1
call :ensure jackett-service.exe || exit /b 1
"%BIN%\jackett-service.exe" --start %ARGS%
exit /b %errorlevel%

:stop
call :require_admin || exit /b 1
call :ensure jackett-service.exe || exit /b 1
"%BIN%\jackett-service.exe" --stop %ARGS%
exit /b %errorlevel%

:status
call :ensure jackett-service.exe || exit /b 1
"%BIN%\jackett-service.exe" --status
exit /b %errorlevel%

:tray
call :ensure jackett-tray.exe || exit /b 1
start "" "%BIN%\jackett-tray.exe" %ARGS%
exit /b 0

:updater
call :ensure jackett-updater.exe || exit /b 1
"%BIN%\jackett-updater.exe" %ARGS%
exit /b %errorlevel%

:check
call :ensure jackett-updater.exe || exit /b 1
"%BIN%\jackett-updater.exe" --check %ARGS%
exit /b %errorlevel%

:launcher
call "%CD%\jackett_launcher.bat"
exit /b %errorlevel%
