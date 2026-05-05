//! Muxer port. Implementations join encoded segments into a single container file.
//!
//! `adapters::ffmpeg_muxer` is the only impl for now (calls `ffmpeg.exe -f concat`).
//! In tests we plug in a mock that records the `inputs` list and writes a stub output file —
//! this keeps the orchestrator e2e tests fast and ffmpeg-binary-free.

use std::path::{Path, PathBuf};

use crate::domain::{Result, SegmentIndex};
use crate::ports::progress_sink::ProgressSink;

/// Newtype for "segment paths in playlist order". Construction via `from_indexed` sorts by
/// `SegmentIndex` — anywhere a `OrderedSegments` value circulates downstream, the order is
/// guaranteed. Muxers can rely on this invariant without re-sorting or re-validating.
#[derive(Debug, Clone)]
pub struct OrderedSegments(Vec<PathBuf>);

impl OrderedSegments {
    /// Sort the (idx, path) pairs by index and discard the indices, leaving an ordered Vec.
    pub fn from_indexed(mut v: Vec<(SegmentIndex, PathBuf)>) -> Self {
        v.sort_by_key(|(idx, _)| *idx);
        Self(v.into_iter().map(|(_, p)| p).collect())
    }
    pub fn paths(&self) -> &[PathBuf] {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[allow(async_fn_in_trait)] // generic over `<M: Muxer>` in the orchestrator, no `dyn`
pub trait Muxer: Send + Sync {
    /// Concatenate `inputs` (in order) into `output`. Reports progress 0..100% via `sink` —
    /// `total_duration_secs` (sum of all `#EXTINF` durations from the playlist) lets us turn
    /// ffmpeg's `out_time_ms` into a percentage.
    async fn mux(
        &self,
        inputs: &OrderedSegments,
        output: &Path,
        total_duration_secs: f64,
        sink: &dyn ProgressSink,
    ) -> Result<()>;
}
