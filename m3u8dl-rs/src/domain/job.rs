use std::path::PathBuf;

use serde::Serialize;

use crate::domain::error::DownloadError;

/// Opaque 8-hex job id (matches the legacy PowerShell server's `[guid]::NewGuid().Substring(0,8)` shape).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct JobId(String);

impl JobId {
    pub fn new() -> Self {
        let raw = uuid::Uuid::new_v4().simple().to_string();
        Self(raw[..8].to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Wrap a string into a JobId without validation. Used by HTTP routes that get the
    /// id from a path segment — caller is responsible for shape; downstream `registry.get()`
    /// returns None on miss anyway.
    pub fn from_string(s: String) -> Self {
        Self(s)
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Newtype to keep segment indices distinct from arbitrary u32s downstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct SegmentIndex(pub u32);

/// Newtype for output mp4 path. Construction via `try_validate` enforces the invariant
/// "file exists on disk and is at least 1024 bytes" — anywhere a `OutputPath` value
/// circulates, this is guaranteed (vs. raw `PathBuf` which carries no such promise).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputPath(pub PathBuf);

impl OutputPath {
    /// Pure constructor: caller stat'd the file, we just enforce the size invariant.
    /// Returns `(self, size_mb)` so the caller can emit Done without re-stat'ing.
    /// Files smaller than 1 KiB are rejected as "ffmpeg produced an invalid empty
    /// container" — this lifts the std::fs side effect out of the domain layer
    /// (audit-architecture-sentinel.domain-io-leak FAIL).
    pub fn try_validate(path: PathBuf, len: u64) -> Result<(Self, f64), DownloadError> {
        if len < 1024 {
            return Err(DownloadError::OutputTooSmall { path, size: len });
        }
        let size_mb = len as f64 / 1_048_576.0;
        Ok((Self(path), size_mb))
    }
}

/// Job lifecycle as a single sum type. Old PS server's `Running/Completed` strings are derived
/// in the DTO layer (see `http::dto`), this enum is the source of truth.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Parsing,
    Downloading { done: u32, total: u32, bytes: u64 },
    Merging { ffmpeg_pct: Option<f32> },
    Done { output: OutputPath, size_mb: f64 },
    Failed { error: String },
}

impl JobState {
    /// Terminal states: no further transitions allowed. Prevents `Failed → Downloading`
    /// regression if a stray progress event arrives after the orchestrator has already
    /// finalized the job.
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Done { .. } | JobState::Failed { .. })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: JobId,
    pub title: String,
    pub state: JobState,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl Job {
    pub fn queued(id: JobId, title: String) -> Self {
        Self {
            id,
            title,
            state: JobState::Queued,
            started_at: chrono::Utc::now(),
        }
    }

    /// Apply a state transition. Returns `false` (and leaves state unchanged) if the
    /// current state is terminal — terminal states never go back to running. All
    /// state writes (orchestrator emit + finalizer) MUST go through this fn — direct
    /// `job.state = …` would bypass the guard.
    pub fn transition_to(&mut self, new: JobState) -> bool {
        if self.state.is_terminal() {
            return false;
        }
        self.state = new;
        true
    }
}
