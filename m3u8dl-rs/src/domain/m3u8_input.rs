//! Discriminated input for the orchestrator: the m3u8 source can be one of
//! three shapes — raw text already present, an HTTP(S) URL to GET, or a local file path.
//!
//! Smart-constructor `detect` is the single entry point — downstream code never branches
//! on string shape, only on the enum variant. This kills the "if-else for kind=ss/wecom/sq"
//! antipattern that the legacy PowerShell `Resolve-InputType` was already trending toward.
//!
//! Effect surfacing: classification is split into a pure `classify_str` (no IO) and
//! `from_classification` which takes an injected file-existence check. `detect` is the
//! convenience wrapper that closes over `Path::is_file` for production callers — domain
//! itself has no `std::fs` calls (audit-architecture-sentinel.domain-io-leak FAIL).

use std::path::{Path, PathBuf};

use url::Url;

use crate::domain::{DownloadError, Result};

/// Pure shape classification of the raw input string. No IO, no filesystem probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    Empty,
    RawM3u8(String),
    Url(Url),
    /// Caller must verify path existence before promoting to `M3u8Input::File`.
    MaybeFile(PathBuf),
}

#[derive(Debug, Clone)]
pub enum M3u8Input {
    /// Already-resolved playlist text (capture.user.js posts the body inline).
    Raw(String),
    /// HTTP(S) URL — fetch and parse.
    Url(Url),
    /// Local file path on disk (CLI / smoke-test use).
    File(PathBuf),
}

/// Pure: classify by shape only. Never touches the filesystem.
pub fn classify_str(s: &str) -> Classification {
    if s.trim().is_empty() {
        return Classification::Empty;
    }
    if s.starts_with("#EXTM3U") {
        return Classification::RawM3u8(s.to_string());
    }
    if let Ok(u) = Url::parse(s)
        && (u.scheme() == "http" || u.scheme() == "https")
    {
        return Classification::Url(u);
    }
    Classification::MaybeFile(PathBuf::from(s))
}

impl M3u8Input {
    /// Promote a `Classification` to `M3u8Input` using the injected file-existence check.
    /// `MaybeFile` is rejected if `file_check(&path)` returns false — keeps the IO effect
    /// in the caller's hands.
    pub fn from_classification(
        c: Classification,
        file_check: impl Fn(&Path) -> bool,
    ) -> Result<Self> {
        match c {
            Classification::Empty => Err(DownloadError::Parse("input is empty".into())),
            Classification::RawM3u8(s) => Ok(Self::Raw(s)),
            Classification::Url(u) => Ok(Self::Url(u)),
            Classification::MaybeFile(p) => {
                if file_check(&p) {
                    Ok(Self::File(p))
                } else {
                    let preview = p.display().to_string().chars().take(80).collect::<String>();
                    Err(DownloadError::Parse(format!(
                        "input is neither #EXTM3U text, nor http(s):// URL, nor existing file (preview: {preview:?})"
                    )))
                }
            }
        }
    }

    /// Convenience wrapper: production callers in HTTP / CLI use this. Closes over
    /// `Path::is_file` as the existence check.
    pub fn detect(s: &str) -> Result<Self> {
        Self::from_classification(classify_str(s), |p| p.is_file())
    }
}
