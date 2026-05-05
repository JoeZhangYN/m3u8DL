//! Server configuration. All knobs are env-overridable for ops use without recompile:
//!
//! | env                          | default                                    |
//! |------------------------------|--------------------------------------------|
//! | `M3U8DL_PORT`                | `7787`                                     |
//! | `M3U8DL_OUT_DIR`             | per-user `<Downloads>/m3u8dl/` (Win SHGetKnownFolderPath / XDG / `./downloads` fallback) |
//! | `M3U8DL_FFMPEG`              | `ffmpeg.exe`                               |
//! | `M3U8DL_PARALLELISM`         | `16`                                       |
//! | `M3U8DL_RETRIES`             | `3`                                        |
//! | `M3U8DL_PROXY`               | (auto-detect WinINET)                      |
//! | `M3U8DL_LOG_FORMAT`          | text (set `json` for JSONL aggregation)    |
//! | `M3U8DL_JOB_DEADLINE_SECS`   | `7200` (2h hard cap on a single job)       |
//! | `M3U8DL_MUX_DEADLINE_FACTOR` | `2.0` (mux deadline = duration × factor + min) |
//! | `M3U8DL_MUX_DEADLINE_MIN_SECS` | `60` (floor for short videos)            |
//!
//! Default HTTP headers here are **site-agnostic** (User-Agent + Accept-Language only).
//! Anti-hotlink `Origin` / `Referer` are NOT hardcoded — `capture.user.js` derives them
//! from `location.href` of the playing page and sends them in the POST body.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;

use reqwest::header::HeaderMap;

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub out_dir: PathBuf,
    pub ffmpeg_path: PathBuf,
    pub parallelism: usize,
    pub max_retries: u32,
    /// Hard cap on total time for a single download job (seconds). Outer `tokio::time::timeout`
    /// in routes.rs wraps the whole orchestrator; on elapse the job is marked Failed and
    /// any in-flight ffmpeg child is dropped (which kills the OS process).
    pub job_deadline_secs: u64,
    /// Mux deadline = `total_duration_secs * factor + min_secs`. Mux is stream copy
    /// (no re-encode), so it should be well under playback duration; factor of 2.0
    /// is generous against IO stalls without being so loose that hung children linger.
    pub mux_deadline_factor: f64,
    pub mux_deadline_min_secs: u64,
    /// Idempotency-Key TTL (seconds). Two POSTs with the same key within this window
    /// return the same JobId. Default 300s = 5min — enough to cover client retry storms
    /// without holding state forever.
    pub idempotency_ttl_secs: u64,
    /// Site-agnostic baseline headers. Per-request `Origin` / `Referer` come from the client.
    pub default_headers: HeaderMap,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: env_or("M3U8DL_PORT", 7787),
            out_dir: env_or_str("M3U8DL_OUT_DIR", &default_out_dir()).into(),
            ffmpeg_path: env_or_str("M3U8DL_FFMPEG", default_ffmpeg_name()).into(),
            parallelism: env_or("M3U8DL_PARALLELISM", 16),
            max_retries: env_or("M3U8DL_RETRIES", 3),
            job_deadline_secs: env_or("M3U8DL_JOB_DEADLINE_SECS", 7200),
            mux_deadline_factor: env_or("M3U8DL_MUX_DEADLINE_FACTOR", 2.0),
            mux_deadline_min_secs: env_or("M3U8DL_MUX_DEADLINE_MIN_SECS", 60),
            idempotency_ttl_secs: env_or("M3U8DL_IDEMPOTENCY_TTL_SECS", 300),
            default_headers: DEFAULT_HEADERS.clone(),
        }
    }
}

fn env_or<T: FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn env_or_str(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

#[cfg(windows)]
const fn default_ffmpeg_name() -> &'static str {
    "ffmpeg.exe"
}
#[cfg(not(windows))]
const fn default_ffmpeg_name() -> &'static str {
    "ffmpeg"
}

/// Pure decision: `Some(downloads)` → append `m3u8dl/` subfolder; `None` → use the provided fallback.
/// No IO, no env reads, no logging — testable by direct injection.
/// `pub(crate)` so the adjacent `#[cfg(test)] mod tests` can call the production fn directly
/// (rather than mirroring its body in tests/, which was the audit's test-gate Critical finding).
pub(crate) fn default_out_dir_from(downloads: Option<PathBuf>, fallback: &Path) -> String {
    match downloads {
        Some(p) => p.join("m3u8dl").to_string_lossy().into_owned(),
        None => fallback.to_string_lossy().into_owned(),
    }
}

/// Per-user Downloads directory plus dedicated `m3u8dl/` subfolder. `dirs::download_dir`
/// resolves to the user's actual configured folder (Windows: `SHGetKnownFolderPath(FOLDERID_Downloads)`,
/// honoring relocation off `%USERPROFILE%`; Linux: `XDG_DOWNLOAD_DIR`; macOS: NSDownloadsDirectory).
/// The `m3u8dl/` suffix isolates downloader output from the user's other Downloads (browsers, etc.).
///
/// Effects: calls `dirs::download_dir()` + `std::env::current_dir()`; emits `tracing::warn!` on fallback.
/// On `None` (very rare; stripped-down profiles or sandboxed runners), anchors fallback to `<cwd>/downloads`
/// (absolute) so a service started from different cwd in different sessions still writes to the same place.
/// If `current_dir()` itself fails (cwd deleted / no permission), bare `./downloads` is the last resort.
fn default_out_dir() -> String {
    if let Some(p) = dirs::download_dir() {
        return default_out_dir_from(Some(p), Path::new("./downloads"));
    }
    let cwd_fallback = std::env::current_dir().map(|c| c.join("downloads"));
    match cwd_fallback {
        Ok(abs) => {
            let resolved = default_out_dir_from(None, &abs);
            tracing::warn!(
                event = "out_dir_fallback",
                reason = "no_downloads_dir_registered",
                path = %resolved,
                "dirs::download_dir() returned None; anchored fallback to cwd"
            );
            resolved
        }
        Err(e) => {
            let resolved = default_out_dir_from(None, Path::new("./downloads"));
            tracing::warn!(
                event = "out_dir_fallback",
                reason = "no_downloads_dir_and_cwd_unavailable",
                path = %resolved,
                error = %e,
                "dirs::download_dir() returned None and current_dir() failed; using bare relative ./downloads"
            );
            resolved
        }
    }
}

#[allow(clippy::expect_used)] // static literal headers — if these don't parse, the source is broken
static DEFAULT_HEADERS: LazyLock<HeaderMap> = LazyLock::new(|| {
    let mut h = HeaderMap::new();
    h.insert(
        "User-Agent",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 Edg/147.0.0.0"
            .parse()
            .expect("UA literal valid"),
    );
    h.insert("Accept", "*/*".parse().expect("Accept literal valid"));
    h.insert(
        "Accept-Language",
        "zh-CN,zh;q=0.9,en;q=0.8"
            .parse()
            .expect("Accept-Language literal valid"),
    );
    h
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_out_dir_from_some_appends_m3u8dl_subdir() {
        let result = default_out_dir_from(Some(PathBuf::from("/tmp/dl")), Path::new("/unused"));
        assert!(result.ends_with("m3u8dl"), "expected suffix m3u8dl, got: {result}");
        assert!(result.starts_with("/tmp/dl"), "expected prefix /tmp/dl, got: {result}");
    }

    #[test]
    fn default_out_dir_from_none_uses_provided_fallback() {
        let result = default_out_dir_from(None, Path::new("/abs/fallback"));
        assert_eq!(result, "/abs/fallback");
    }

    #[test]
    fn default_out_dir_from_handles_unicode_path() {
        let result = default_out_dir_from(
            Some(PathBuf::from("/Users/张三/Downloads")),
            Path::new("/unused"),
        );
        assert!(result.contains("张三"), "Unicode round-trip failed: {result}");
        assert!(result.ends_with("m3u8dl"), "expected suffix m3u8dl, got: {result}");
    }
}
