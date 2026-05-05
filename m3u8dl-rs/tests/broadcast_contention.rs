//! Regression test for `BroadcastSink::emit` under mutex contention.
//!
//! Plan commit 2 (Tier B C1 fix) replaced `try_lock` with `lock_or_poisoned` to eliminate
//! silent state-mirror drops under contention. Without this test, a regression to `try_lock`
//! would silently re-introduce the bug. Test strategy:
//!   1. Hold the Job's mutex from a separate (sync) task for ~50ms.
//!   2. Concurrently call `emit()` from the main task; verify it eventually completes
//!      (after the holder releases) and the Job.state reflects the new state.
//!   3. Wrap in `tokio::time::timeout(2s)` so a regression to `try_lock` (which would
//!      cause silent drop, leaving state == Queued) is detectable.

// Note: this test deliberately uses `Mutex::lock().expect("test")` instead of the
// production `lock_or_poisoned` helper because that helper is `pub(crate)` and not
// reachable from integration tests. `expect` in test code is allowed by clippy.toml.

#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use m3u8dl_server::adapters::broadcast_sink::BroadcastSink;
use m3u8dl_server::domain::{Job, JobId, JobState, ProgressEvent};
use m3u8dl_server::ports::progress_sink::ProgressSink;

#[tokio::test]
async fn emit_blocks_during_contention_then_writes_state() {
    let job_id = JobId::new();
    let job = Arc::new(Mutex::new(Job::queued(job_id, "test".into())));
    let sink = BroadcastSink::new(16, job.clone());

    // Holder thread: acquire the mutex and sleep, simulating a polling /status read
    // that happens to overlap with emit().
    let holder_job = job.clone();
    let holder = std::thread::spawn(move || {
        let _guard = holder_job.lock().expect("test mutex");
        std::thread::sleep(Duration::from_millis(50));
        // guard drops here, releasing the mutex
    });

    // Give the holder a head start so emit() definitely encounters contention
    tokio::time::sleep(Duration::from_millis(5)).await;

    let event = ProgressEvent::Parsing;
    tokio::time::timeout(Duration::from_secs(2), async {
        // emit is sync but calls std::sync::Mutex::lock() which blocks the OS thread.
        // Wrap in spawn_blocking so the tokio runtime stays responsive even if emit
        // genuinely hangs (regression to try_lock would NOT hang — would silently
        // skip the state write, which we catch via the post-emit assertion below).
        let sink = sink.clone();
        tokio::task::spawn_blocking(move || sink.emit(event)).await
    })
    .await
    .expect("emit timed out — possible regression to try_lock would not hang though")
    .expect("spawn_blocking join");

    // Wait for the holder to finish so we can lock for our final assertion.
    holder.join().expect("holder thread panicked");

    // Final assertion: state was actually written. If emit had used try_lock and
    // silently skipped (the bug Plan commit 2 fixed), state would still be Queued.
    let state_snapshot = {
        let g = job.lock().expect("test mutex");
        g.state.clone()
    };
    assert!(
        matches!(state_snapshot, JobState::Parsing),
        "emit() under contention failed to write state — got {state_snapshot:?} (try_lock regression?)"
    );
}

#[tokio::test]
async fn emit_writes_state_without_contention() {
    // Sanity check: ensure the test infrastructure works in the no-contention case.
    let job_id = JobId::new();
    let job = Arc::new(Mutex::new(Job::queued(job_id, "test".into())));
    let sink = BroadcastSink::new(16, job.clone());

    sink.emit(ProgressEvent::Parsing);
    let g = job.lock().expect("test mutex");
    assert!(matches!(g.state, JobState::Parsing));
}
