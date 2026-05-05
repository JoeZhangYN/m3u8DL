//! Per-job manifest written to `work_dir/progress.json` after segment downloads complete,
//! before mux. Failure paths preserve work_dir + manifest for post-mortem; success path
//! removes work_dir (so manifest is short-lived in normal operation).
//!
//! Schema: `version: 1` discriminator so future readers can branch on layout. Today's
//! readers (D2 resume code) just check version == 1; mismatch → ignore.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::Result;

pub const MANIFEST_VERSION: u32 = 1;
pub const MANIFEST_FILENAME: &str = "progress.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgressManifest {
    pub version: u32,
    pub job_id: String,
    pub url: Option<String>,
    /// Sorted, deduplicated segment indices that were downloaded. Smart-ctor
    /// `try_new` sorts + dedups + asserts len <= total.
    pub segments_downloaded: Vec<u32>,
    pub total: u32,
    pub written_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("segments_downloaded len {got} exceeds total {total}")]
    LenExceedsTotal { got: usize, total: u32 },
}

impl ProgressManifest {
    /// Smart constructor: sorts + dedups segments_downloaded, asserts len <= total.
    pub fn try_new(
        job_id: String,
        url: Option<String>,
        mut segments_downloaded: Vec<u32>,
        total: u32,
    ) -> std::result::Result<Self, ManifestError> {
        segments_downloaded.sort_unstable();
        segments_downloaded.dedup();
        if segments_downloaded.len() > total as usize {
            return Err(ManifestError::LenExceedsTotal {
                got: segments_downloaded.len(),
                total,
            });
        }
        Ok(Self {
            version: MANIFEST_VERSION,
            job_id,
            url,
            segments_downloaded,
            total,
            written_at: Utc::now(),
        })
    }

    /// Write atomically: serialize → tmp file → rename. Failure to write is non-fatal
    /// (manifest is diagnostic; the actual download is unaffected).
    pub async fn write_to_dir(&self, dir: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| {
            crate::domain::DownloadError::Parse(format!("manifest serialize: {e}"))
        })?;
        let target = dir.join(MANIFEST_FILENAME);
        let tmp = dir.join(format!("{MANIFEST_FILENAME}.tmp"));
        tokio::fs::write(&tmp, json).await?;
        tokio::fs::rename(&tmp, &target).await?;
        Ok(())
    }

    /// Read + parse the manifest from `dir/progress.json`. Returns Ok(None) if file
    /// doesn't exist or version mismatch (caller falls back to no-resume).
    pub async fn read_from_dir(dir: &Path) -> Result<Option<Self>> {
        let path = dir.join(MANIFEST_FILENAME);
        let bytes = match tokio::fs::read(&path).await {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let manifest: Self = match serde_json::from_slice(&bytes) {
            Ok(m) => m,
            Err(_) => return Ok(None), // corrupt manifest → ignore, restart fresh
        };
        if manifest.version != MANIFEST_VERSION {
            return Ok(None); // version mismatch → ignore
        }
        Ok(Some(manifest))
    }
}
