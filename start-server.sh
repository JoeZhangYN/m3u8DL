#!/usr/bin/env bash
# Linux / macOS launcher for m3u8dl-server. Equivalent of start-server.bat on Windows.
set -e
cd "$(dirname "$0")"
echo
echo "=== m3u8dl-server ==="
echo "Listening on http://127.0.0.1:7787"
echo "Press Ctrl+C to stop"
echo
exec ./m3u8dl-server
