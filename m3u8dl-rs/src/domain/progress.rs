use std::path::PathBuf;

use serde::Serialize;

use crate::domain::{JobState, OutputPath};

/// Progress event pushed via `tokio::sync::broadcast` to all SSE subscribers of a job.
///
/// JSON shape (snake_case `phase` discriminator) is the SOT for the SSE wire format —
/// `capture.user.js` will read this directly in PR 10.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ProgressEvent {
    Parsing,
    Downloading {
        done: u32,
        total: u32,
        bytes: u64,
        seg_index: u32,
    },
    Merging {
        pct: Option<f32>,
    },
    Done {
        output: PathBuf,
        size_mb: f64,
    },
    Failed {
        error: String,
    },
}

impl ProgressEvent {
    /// Convenience ctor — keeps construction in one place (SOT, see CLAUDE.md §3.2).
    pub fn downloading(done: u32, total: u32, bytes: u64, seg_index: u32) -> Self {
        Self::Downloading { done, total, bytes, seg_index }
    }
    pub fn merging(pct: Option<f32>) -> Self { Self::Merging { pct } }
    pub fn done(output: PathBuf, size_mb: f64) -> Self { Self::Done { output, size_mb } }
    pub fn failed(error: impl Into<String>) -> Self { Self::Failed { error: error.into() } }

    /// Lift a progress event into the persisted `JobState` so polling clients (`GET /job/:id`)
    /// see the same detail as SSE subscribers. Single-source mapping — both broadcast push and
    /// polling derive their UI from this conversion.
    pub fn to_job_state(&self) -> JobState {
        match self {
            Self::Parsing => JobState::Parsing,
            Self::Downloading { done, total, bytes, .. } => {
                JobState::Downloading { done: *done, total: *total, bytes: *bytes }
            }
            Self::Merging { pct } => JobState::Merging { ffmpeg_pct: *pct },
            Self::Done { output, size_mb } => {
                JobState::Done { output: OutputPath(output.clone()), size_mb: *size_mb }
            }
            Self::Failed { error } => JobState::Failed { error: error.clone() },
        }
    }
}
