# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2026-05-05

### Added
- **Structured startup event** — `tracing::info!(event="server_started", version, port, out_dir)` emitted alongside the human-readable stdout banner; log aggregation systems can now key on `event` field.
- **`M3U8DL_LOG_FORMAT=json` env toggle** — when set to `json`, `tracing-subscriber` switches from text formatter to JSON output (one event per line, machine-grep friendly). Default unchanged (text).
- **Fallback warning event** — when `dirs::download_dir()` returns `None`, `tracing::warn!(event="out_dir_fallback", reason, path)` is emitted instead of silently using bare `./downloads`. Helps diagnose "files landed somewhere unexpected" reports on stripped-down profiles / sandboxed runners.

### Changed
- **`default_out_dir` internal refactor** — split into pure decision fn `default_out_dir_from(Option<PathBuf>, &Path) -> String` (testable by injection) plus an impure wrapper that resolves `dirs::download_dir()` + cwd. Behavior change: when `dirs::download_dir()` returns `None`, fallback is now anchored to `<cwd>/downloads` (absolute) rather than the bare relative `./downloads` — services started from different cwd in different sessions now consistently target the same directory. Bare relative `./downloads` remains as a last-resort fallback if `current_dir()` itself fails.
- **Startup `create_dir_all` failure** now uses `tracing::error!(event="startup_aborted", rule="out_dir_unwritable", path, error)` instead of `eprintln! "FATAL: ..."`. The process still exits with code 2 (no behavioral regression), but the error now goes through the tracing pipeline so log aggregation captures it.
- **TCP `bind` failure** wraps the `?` propagation with a `tracing::error!(event="bind_failed", port, error)` event before returning to `anyhow`.

### Fixed
- **`docs/API.md`** gained a non-authoritative SOT pointer comment at the top, pointing port `7787` and JSON shape definitions back to `src/config.rs::Config::default` and `src/http/dto.rs`. Same for `docs/SCOPE.md` retry timings → `src/adapters/reqwest_client.rs`.

## [0.1.1] - 2026-05-04

### Added
- **Startup banner now prints `output directory: <path>`** for visibility (so users can see at a glance where downloads will land). _(Backfilled in 0.1.2: this user-visible stdout change shipped in 0.1.1 but was missing from the original 0.1.1 changelog entry.)_

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
