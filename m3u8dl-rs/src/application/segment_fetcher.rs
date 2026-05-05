// error-chain: exempt — IO failures here are wrapped in DownloadError::Io which already
// captures kind+os-msg; segment URL is logged via tracing in the orchestrator one frame up.

//! Fetch one HLS segment: download, decrypt if AES-128, write to disk.
//! Key bytes are cached per URI (HLS spec allows the same key URI to back many segments;
//! re-downloading would multiply request count by N).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use reqwest::header::HeaderMap;
use tokio::sync::RwLock;
use url::Url;

use crate::domain::codec::{aes_decrypt, png_strip};
use crate::domain::{DownloadError, Encryption, Result, Segment, SegmentIndex};
use crate::ports::http_client::{HttpClient, HttpRequest};
use crate::util::url_redact::redact_url;

pub struct FetchedSegment {
    pub idx: SegmentIndex,
    pub path: PathBuf,
    pub bytes_written: u64,
}

pub type KeyCache = Arc<RwLock<HashMap<Url, Bytes>>>;

pub fn new_key_cache() -> KeyCache {
    Arc::new(RwLock::new(HashMap::new()))
}

pub async fn fetch_one<C: HttpClient>(
    seg: Segment,
    http: &C,
    key_cache: &KeyCache,
    headers: &HeaderMap,
    work_dir: &Path,
) -> Result<FetchedSegment> {
    let path = work_dir.join(format!("seg-{:06}.ts", seg.idx.0));
    // Resume hook (Plan D2): if the segment file already exists with non-zero size from a
    // prior run that crashed mid-job, skip the fetch + decrypt + write. Saves bandwidth on
    // retry of partial downloads. Conservative check (size > 0) — a 0-byte file is treated
    // as a half-write artifact and re-fetched. Future hardening: cross-check against
    // progress.json manifest sizes (Plan D3a/D3b ground); today's check is best-effort
    // and assumes the file is sound if it's non-empty (typical for completed segments).
    if let Ok(meta) = tokio::fs::metadata(&path).await
        && meta.is_file()
        && meta.len() > 0
    {
        return Ok(FetchedSegment {
            idx: seg.idx,
            path,
            bytes_written: meta.len(),
        });
    }

    let req = HttpRequest::new(seg.url.clone()).with_headers(headers.clone());
    let req = match seg.byte_range {
        Some(br) => {
            let start = br.offset.unwrap_or(0);
            let end = start.saturating_add(br.length).saturating_sub(1);
            req.with_range(start, end)
        }
        None => req,
    };
    let raw = http.fetch_bytes(req).await?;
    // Some anti-bot sites wrap segment bytes in a PNG envelope (real PNG signature + trailing
    // TS/m4s data after IEND). Strip before AES decrypt so the cipher sees clean ciphertext,
    // and before writing to disk so ffmpeg sees a real container instead of a 1x1 PNG stream.
    let stripped = png_strip::strip_png_wrapper(&raw);

    let plaintext = match seg.encryption {
        Encryption::None => stripped.to_vec(),
        Encryption::Aes128Cbc { key_uri, iv } => {
            let key_bytes = resolve_key(http, key_cache, &key_uri, headers).await?;
            if key_bytes.len() != 16 {
                return Err(DownloadError::Decrypt(format!(
                    "key from {} is {} bytes, expected 16",
                    redact_url(&key_uri),
                    key_bytes.len()
                )));
            }
            let mut key = [0u8; 16];
            key.copy_from_slice(&key_bytes);
            aes_decrypt::decrypt(stripped, &key, &iv)?
        }
    };

    tokio::fs::write(&path, &plaintext).await?;
    let bytes_written = plaintext.len() as u64;
    Ok(FetchedSegment {
        idx: seg.idx,
        path,
        bytes_written,
    })
}

async fn resolve_key<C: HttpClient>(
    http: &C,
    cache: &KeyCache,
    uri: &Url,
    headers: &HeaderMap,
) -> Result<Bytes> {
    if let Some(b) = cache.read().await.get(uri).cloned() {
        return Ok(b);
    }
    let bytes = http
        .fetch_bytes(HttpRequest::new(uri.clone()).with_headers(headers.clone()))
        .await?;
    cache.write().await.insert(uri.clone(), bytes.clone());
    Ok(bytes)
}
