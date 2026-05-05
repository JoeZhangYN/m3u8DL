//! HTTP wire DTOs. **Source of truth for the JSON contract** with `capture.user.js` —
//! field names here MUST stay in sync with the Tampermonkey script (and any future client).
//! Old PS server's flat string state (`Queued|Running|Completed`) is preserved here so the
//! existing JS polling loop keeps working unchanged.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::domain::{Job, JobState};

#[derive(Debug, Deserialize)]
pub struct DownloadRequestBody {
    /// The actual m3u8 URL the browser saw (used for base-URL derivation).
    #[serde(default)]
    pub url: Option<String>,
    /// Inline m3u8 text (capture.user.js posts the response body here).
    pub m3u8: String,
    /// Display-name / output filename hint.
    #[serde(default)]
    pub title: Option<String>,
    /// Originating page URL — used as a fallback to derive Referer/Origin if `headers`
    /// is empty.
    #[serde(default)]
    pub page: Option<String>,
    /// Client-supplied request headers (e.g. `Origin`, `Referer`, `Cookie`). These are
    /// merged on top of the server's site-agnostic defaults — the browser knows the right
    /// anti-hotlink headers because it is on the playing page; we just forward them.
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
pub struct DownloadAccepted {
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub status: &'static str,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct PingResponse {
    pub ok: bool,
    pub port: u16,
    pub jobs: usize,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub jobs: Vec<JobSnapshot>,
}

/// Snapshot identical in shape to the legacy PS server's job snapshot, **plus** a new
/// optional `progress` field carrying per-segment / per-percentage detail. Old JS that
/// only reads `state` keeps working; new JS reads `progress` for the live UI.
#[derive(Debug, Serialize)]
pub struct JobSnapshot {
    pub id: String,
    pub title: String,
    pub state: &'static str,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    pub success: Option<bool>,
    pub output: Option<String>,
    #[serde(rename = "sizeMB")]
    pub size_mb: Option<f64>,
    pub error: Option<String>,
    /// Optional fine-grained progress detail. Only present during Parsing/Downloading/Merging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ProgressDetail>,
}

/// Mirrors `ProgressEvent` (snake_case `phase` discriminator) — parallel JSON shape so the
/// polling UI and the SSE UI use the same parser. SOT for the wire shape is here.
#[derive(Debug, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ProgressDetail {
    Parsing,
    Downloading { done: u32, total: u32, bytes: u64 },
    Merging { pct: Option<f32> },
}

impl JobSnapshot {
    pub fn from_job(job: &Job) -> Self {
        let (state_str, success, output, size_mb, error, progress) = match &job.state {
            JobState::Queued => ("Queued", None, None, None, None, None),
            JobState::Parsing => (
                "Running",
                None,
                None,
                None,
                None,
                Some(ProgressDetail::Parsing),
            ),
            JobState::Downloading { done, total, bytes } => (
                "Running",
                None,
                None,
                None,
                None,
                Some(ProgressDetail::Downloading {
                    done: *done,
                    total: *total,
                    bytes: *bytes,
                }),
            ),
            JobState::Merging { ffmpeg_pct } => (
                "Running",
                None,
                None,
                None,
                None,
                Some(ProgressDetail::Merging { pct: *ffmpeg_pct }),
            ),
            JobState::Done { output, size_mb } => (
                "Completed",
                Some(true),
                Some(output.0.display().to_string()),
                Some(*size_mb),
                None,
                None,
            ),
            JobState::Failed { error } => (
                "Completed",
                Some(false),
                None,
                None,
                Some(error.clone()),
                None,
            ),
        };
        Self {
            id: job.id.as_str().to_string(),
            title: job.title.clone(),
            state: state_str,
            started_at: job.started_at.to_rfc3339(),
            success,
            output,
            size_mb,
            error,
            progress,
        }
    }
}
