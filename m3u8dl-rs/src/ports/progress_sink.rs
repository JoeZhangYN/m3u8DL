//! Progress event sink port. Implementations:
//! - `adapters::broadcast_sink::BroadcastSink` (PR 6) — fans events out to SSE subscribers
//! - test-only sinks (e.g. `Vec<ProgressEvent>` recorder via `Mutex`)
//!
//! Single-method trait kept tiny — fan-out / batching live in adapters, not here.

use crate::domain::ProgressEvent;

pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: ProgressEvent);
}
