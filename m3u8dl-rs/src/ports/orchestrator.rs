//! Download orchestrator port. Hides the concrete `DownloadJob<C, M>` so that `AppState`
//! can hold `Arc<dyn DownloadOrchestrator>` and tests can inject a stub orchestrator
//! without forcing a dummy ffmpeg binary path on disk (audit DIP fix — plan §4.3).
//!
//! Why a separate trait (vs reusing `Muxer` / `HttpClient` / `ProgressSink`):
//!   * No existing port has the `run(req, sink) -> (OutputPath, f64)` shape — they all
//!     model narrower IO concerns.
//!   * `AppState.job: Arc<dyn DownloadOrchestrator>` requires dyn compatibility, which
//!     the existing ports (with native `async fn in trait`) don't provide.
//!
//! `async-trait` is the standard escape hatch for `dyn` async traits on stable Rust 1.87.

use std::sync::Arc;

use async_trait::async_trait;

use crate::application::download_job::DownloadRequest;
use crate::domain::{OutputPath, Result};
use crate::ports::progress_sink::ProgressSink;

#[async_trait]
pub trait DownloadOrchestrator: Send + Sync {
    /// Run a single download to completion. Returns `(validated_output_path, size_mb)`
    /// on success; `DownloadError` on any failure. Cancellation is upstream — caller
    /// (HTTP layer) wraps this future in `tokio::time::timeout` for outer deadline.
    async fn run(
        &self,
        req: DownloadRequest,
        sink: Arc<dyn ProgressSink>,
    ) -> Result<(OutputPath, f64)>;
}
