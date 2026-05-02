@echo off
chcp 65001 >nul
cd /d "%~dp0"
title m3u8dl-server (127.0.0.1:7787)
echo.
echo === m3u8dl-server ===
echo Listening on http://127.0.0.1:7787
echo Press Ctrl+C to stop
echo.
m3u8dl-server.exe
echo.
echo Server stopped.
pause
