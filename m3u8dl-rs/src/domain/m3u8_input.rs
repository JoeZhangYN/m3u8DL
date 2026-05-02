//! Discriminated input for the orchestrator: the m3u8 source can be one of
//! three shapes — raw text already present, an HTTP(S) URL to GET, or a local file path.
//!
//! Smart-constructor `detect` is the single entry point — downstream code never branches
//! on string shape, only on the enum variant. This kills the "if-else for kind=ss/wecom/sq"
//! antipattern that the legacy PowerShell `Resolve-InputType` was already trending toward.

use std::path::PathBuf;

use url::Url;

use crate::domain::{DownloadError, Result};

#[derive(Debug, Clone)]
pub enum M3u8Input {
    /// Already-resolved playlist text (capture.user.js posts the body inline).
    Raw(String),
    /// HTTP(S) URL — fetch and parse.
    Url(Url),
    /// Local file path on disk (CLI / smoke-test use).
    File(PathBuf),
}

impl M3u8Input {
    /// Classify a raw input string. Detection rules (matching legacy `pipeline.ps1`):
    /// - starts with `#EXTM3U` → `Raw`
    /// - parses as `http://` or `https://` URL → `Url`
    /// - existing local file path → `File`
    /// - else → `DownloadError::Parse(..)`
    pub fn detect(s: &str) -> Result<Self> {
        if s.trim().is_empty() {
            return Err(DownloadError::Parse("input is empty".into()));
        }
        if s.starts_with("#EXTM3U") {
            return Ok(Self::Raw(s.to_string()));
        }
        if let Ok(u) = Url::parse(s)
            && (u.scheme() == "http" || u.scheme() == "https")
        {
            return Ok(Self::Url(u));
        }
        let p = PathBuf::from(s);
        if p.is_file() {
            return Ok(Self::File(p));
        }
        let preview: String = s.chars().take(80).collect();
        Err(DownloadError::Parse(format!(
            "input is neither #EXTM3U text, nor http(s):// URL, nor existing file (preview: {preview:?})"
        )))
    }
}
