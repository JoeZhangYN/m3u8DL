//! Server configuration. All knobs are env-overridable for ops use without recompile:
//!
//! | env                    | default                  |
//! |------------------------|--------------------------|
//! | `M3U8DL_PORT`          | `7787`                   |
//! | `M3U8DL_OUT_DIR`       | `C:\Folder\Download`     |
//! | `M3U8DL_FFMPEG`        | `ffmpeg.exe`             |
//! | `M3U8DL_PARALLELISM`   | `16`                     |
//! | `M3U8DL_RETRIES`       | `3`                      |
//! | `M3U8DL_PROXY`         | (auto-detect WinINET)    |
//!
//! Default HTTP headers here are **site-agnostic** (User-Agent + Accept-Language only).
//! Anti-hotlink `Origin` / `Referer` are NOT hardcoded — `capture.user.js` derives them
//! from `location.href` of the playing page and sends them in the POST body.

use std::path::PathBuf;
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
            default_headers: DEFAULT_HEADERS.clone(),
        }
    }
}

fn env_or<T: FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn env_or_str(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| default.to_string())
}

#[cfg(windows)]
const fn default_ffmpeg_name() -> &'static str { "ffmpeg.exe" }
#[cfg(not(windows))]
const fn default_ffmpeg_name() -> &'static str { "ffmpeg" }

fn default_out_dir() -> String {
    #[cfg(windows)]
    {
        r"C:\Folder\Download".to_string()
    }
    #[cfg(not(windows))]
    {
        // ~/Downloads/m3u8dl on linux/macOS, fall back to ./downloads if HOME is unset
        match std::env::var("HOME") {
            Ok(home) if !home.is_empty() => format!("{home}/Downloads/m3u8dl"),
            _ => "./downloads".to_string(),
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
        "zh-CN,zh;q=0.9,en;q=0.8".parse().expect("Accept-Language literal valid"),
    );
    h
});
