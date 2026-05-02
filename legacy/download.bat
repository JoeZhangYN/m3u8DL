@echo off
cd /d "%~dp0"
title M3U8 Downloader (PNG-wrapped)
where pwsh >nul 2>&1
if %errorlevel%==0 (
    pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0download.ps1"
) else (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0download.ps1"
)
echo.
pause