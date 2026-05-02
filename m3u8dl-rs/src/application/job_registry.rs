//! In-memory registry of running / completed jobs. One `JobHandle` per `JobId`.
//!
//! Concurrency: `DashMap` shards locks across keys, so SSE subscribers on `/events/{a}`
//! never block POST `/download` creating job `b`. Each `JobHandle` carries its own broadcast
//! channel — dropping the handle (job removed from registry) closes all subscribers cleanly.
//!
//! `Job` itself uses `std::sync::Mutex` (not tokio's): the lock is held for nanoseconds
//! during state writes and reads, never across `.await`. This lets the broadcast sink's
//! sync `emit()` write the new state without wrapping itself in `block_on`.

use std::sync::{Arc, Mutex};

use dashmap::DashMap;

use crate::adapters::broadcast_sink::BroadcastSink;
use crate::domain::{Job, JobId, JobState};

#[derive(Clone)]
pub struct JobHandle {
    pub job: Arc<Mutex<Job>>,
    pub sink: BroadcastSink,
}

#[derive(Clone, Default)]
pub struct JobRegistry {
    inner: Arc<DashMap<JobId, JobHandle>>,
}

impl JobRegistry {
    pub fn new() -> Self { Self::default() }

    /// Allocate a new job slot. The returned `JobHandle.sink` is wired to update
    /// `JobHandle.job.state` on every `emit()` (so polling clients see the same detail
    /// as SSE subscribers — single source of truth via `ProgressEvent::to_job_state`).
    pub fn register(&self, id: JobId, title: String) -> JobHandle {
        let job = Arc::new(Mutex::new(Job::queued(id.clone(), title)));
        let sink = BroadcastSink::new(256, job.clone());
        let handle = JobHandle { job, sink };
        self.inner.insert(id, handle.clone());
        handle
    }

    pub fn get(&self, id: &JobId) -> Option<JobHandle> {
        self.inner.get(id).map(|r| r.clone())
    }

    pub fn count(&self) -> usize { self.inner.len() }

    pub fn ids(&self) -> Vec<JobId> {
        self.inner.iter().map(|r| r.key().clone()).collect()
    }

    /// Apply a state transition to an existing job. No-op if id unknown or current
    /// state is terminal (see `Job::transition_to`).
    #[allow(clippy::expect_used)] // poisoned mutex = bug; see lock_or_poisoned pattern
    pub fn set_state(&self, id: &JobId, state: JobState) {
        if let Some(h) = self.get(id) {
            let mut g = h.job.lock().expect("job mutex poisoned");
            g.transition_to(state);
        }
    }
}
