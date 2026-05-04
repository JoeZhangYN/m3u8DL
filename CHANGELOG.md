# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-05-04

### Changed
- **Default output directory now resolves to `<Downloads>/m3u8dl/` on all platforms.**
  - Windows: `SHGetKnownFolderPath(FOLDERID_Downloads)/m3u8dl`
  - Linux: `$XDG_DOWNLOAD_DIR/m3u8dl`
  - macOS: `NSDownloadsDirectory/m3u8dl`
  - Fallback (no Downloads dir registered): `./downloads`
- The `m3u8dl/` suffix isolates downloader output from the user's other Downloads (browsers, etc.) and is symmetric across all platforms.
- README, `docs/API.md`, `docs/SCOPE.md` doc copies synced to the new default.

### Fixed
- `m3u8dl-rs/docs/API.md` `/status` example: stale `C:\Folder\Download\xxx.mp4` literal replaced with `<out_dir>/xxx.mp4` placeholder.

### Migration
The `M3U8DL_OUT_DIR` env var override is unchanged. To restore previous behavior:

| Previous behavior | How to restore in 0.1.1 |
|-------------------|--------------------------|
| Windows v0.1.0 hardcoded `C:\Folder\Download` | `set M3U8DL_OUT_DIR=C:\Folder\Download` |
| Bare `<Downloads>/` (interim, between v0.1.0 and v0.1.1) | `set M3U8DL_OUT_DIR=%USERPROFILE%\Downloads` (Windows) / `export M3U8DL_OUT_DIR="$HOME/Downloads"` (Linux/macOS) |
| Linux/macOS v0.1.0 `~/Downloads/m3u8dl/` | _no override needed — now the default_ |

## [0.1.0] - 2026-05-02

### Added
- Initial public release: zero-config m3u8 downloader with HTTP server, SSE progress, ffmpeg muxing, AES-128-CBC decrypt, PNG-wrapper segment stripping.
