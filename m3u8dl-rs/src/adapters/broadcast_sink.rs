//! `ProgressSink` impl that does double-write: broadcast (for SSE subscribers) +
//! `Job.state` mutation (for polling clients via `GET /job/:id`).
//!
//! Single source of truth for the state mapping is `ProgressEvent::to_job_state` in
//! `domain/progress.rs` — both the SSE wire format and the polling JSON derive from there.

use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::application::job_registry::lock_or_poisoned;
use crate::domain::{Job, ProgressEvent};
use crate::ports::progress_sink::ProgressSink;

#[derive(Clone)]
pub struct BroadcastSink {
    sender: broadcast::Sender<ProgressEvent>,
    /// Mirror target — every `emit()` writes the derived `JobState` here so
    /// `GET /job/:id` polling sees per-segment progress without subscribing to SSE.
    job: Arc<Mutex<Job>>,
}

impl BroadcastSink {
    pub fn new(capacity: usize, job: Arc<Mutex<Job>>) -> Self {
        let (sender, _rx) = broadcast::channel(capacity);
        Self { sender, job }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ProgressEvent> {
        self.sender.subscribe()
    }
}

impl ProgressSink for BroadcastSink {
    fn emit(&self, event: ProgressEvent) {
        // 1. mirror to Job.state for polling. Goes through `transition_to` — terminal
        //    states block further mutations (defense vs late stray events).
        //    Use `lock_or_poisoned` (blocking) instead of `try_lock`: prior `try_lock`
        //    silently dropped the mirror under contention, leaving polling clients with
        //    stale state. The lock is held for nanoseconds and never crosses `.await`.
        let mut g = lock_or_poisoned(&self.job);
        g.transition_to(event.to_job_state());
        drop(g);
        // 2. broadcast for SSE — `Err` only fires when there are no receivers, ignore.
        drop(self.sender.send(event));
    }
}
