//! Derive a base URL for resolving relative segment URIs from a fetched m3u8.
//!
//! Why a dedicated fn (instead of feeding `m3u8_url` straight into `parser::parse_m3u8`):
//! `Url::join` has surprising semantics for paths *without* trailing slash. Given
//! `https://cdn.example.com/v/index.m3u8`, joining `seg-0.ts` yields
//! `https://cdn.example.com/v/seg-0.ts` — but only if the trailing path is treated as a
//! "directory". `Url::join` actually does this correctly when the path ends with `/`
//! (it strips the final segment otherwise), so we must explicitly pop the `.m3u8` filename
//! and ensure a trailing slash. Query / fragment are stripped (they belong to the playlist
//! request, not to the segments).
//!
//! This module exists because the original PowerShell prototype shipped a bug fix around
//! exactly this — see RUST_PORT_REFERENCE.md §3 ("base-url 推导").

use url::Url;

use crate::domain::{DownloadError, Result};

/// Strip the m3u8 filename + query + fragment, leaving a `scheme://host[:port]/path/` URL
/// that resolves relative segment URIs the way browsers / RE / N_m3u8DL-CLI do.
pub fn derive_base_url(m3u8_url: &Url) -> Result<Url> {
    let mut base = m3u8_url.clone();
    base.set_query(None);
    base.set_fragment(None);
    base.path_segments_mut()
        .map_err(|()| DownloadError::Parse(format!("URL '{m3u8_url}' cannot be a base")))?
        .pop()
        .push(""); // ensure trailing '/'
    Ok(base)
}
