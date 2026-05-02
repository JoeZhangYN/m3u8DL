# Legacy PowerShell prototype

These files are the original PowerShell prototype that the new `m3u8dl-server.exe` replaces.

| File | Purpose |
|------|---------|
| `server.bat` | Starts the PowerShell HTTP server |
| `download_server.ps1` | HTTP listener (`HttpListener` on 7787) |
| `pipeline.ps1` | PNG strip + m3u8 normalize + N_m3u8DL-RE.exe + ffmpeg pipeline |
| `download.bat` / `download.ps1` | Standalone CLI download (no server) |
| `N_m3u8DL-RE-v0.5.1-beta.exe` | Underlying downloader the PS pipeline calls |
| `N_m3u8DL-CLI_v3.0.2.exe` | Older CLI variant |
| `N_m3u8DL-CLI-SimpleG.exe` | Even older CLI variant |

## Why kept

If `m3u8dl-server.exe` regresses on a niche source site we haven't tested, you can fall back to:

```cmd
cd legacy
server.bat
```

Note: the PS pipeline expects `N_m3u8DL-RE-v0.5.1-beta.exe` to live next to itself
(now in this `legacy/` directory) and `ffmpeg.exe` in the project root.

Tampermonkey's `capture.user.js` POSTs to `127.0.0.1:7787` either way — only one of the two
servers can run at a time on that port.

## Binaries not in git

`N_m3u8DL-RE-v0.5.1-beta.exe` and the `N_m3u8DL-CLI-*.exe` binaries are `.gitignore`'d
(too large + license unclear). To use the PS fallback you need to drop them in this dir
yourself — get the latest from https://github.com/nilaoda/N_m3u8DL-RE/releases.

## Removal

Once `m3u8dl-server.exe` has been stable for one release cycle (~2-4 weeks of real downloads),
this whole directory can be deleted.
