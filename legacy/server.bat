@echo off
cd /d "%~dp0"
title M3U8 Download Server (127.0.0.1:7787)
where pwsh >nul 2>&1
if %errorlevel%==0 (
    pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0download_server.ps1"
) else (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0download_server.ps1"
)
echo.
echo Server stopped.
pause