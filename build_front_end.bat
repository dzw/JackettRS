@echo off
rem Build the Jackett (Rust) built-in web UI.
rem
rem Runs the Vue project in jackett-frontend\ and writes the single-file
rem bundle to jackett-server\wwwroot\index.html, which the Rust server
rem picks up via ServeDir and compiles into the binary with include_str!.
rem
rem Requires Node.js (node + npm) on PATH.

setlocal EnableExtensions
cd /d "%~dp0jackett-frontend"

where npm >nul 2>&1
if errorlevel 1 (
  echo npm not found on PATH. Install Node.js from https://nodejs.org/
  exit /b 1
)

if not exist "node_modules" (
  echo Installing frontend dependencies...
  call npm install
  if errorlevel 1 (
    echo npm install failed.
    exit /b 1
  )
)

echo Building frontend into ..\jackett-server\wwwroot ...
call npm run build
if errorlevel 1 (
  echo Frontend build failed.
  exit /b 1
)

echo.
echo Done: jackett-server\wwwroot\index.html updated.
echo Rebuild the Rust workspace ^(run.bat build^) to embed it into the binary.
exit /b 0
