//! Verify Job state machine transition guard:
//! terminal states (Done / Failed) never go back to running.

use std::path::PathBuf;

use m3u8dl_server::domain::{Job, JobId, JobState, OutputPath};

fn new_job() -> Job {
    Job::queued(JobId::new(), "test".into())
}

#[test]
fn queued_can_transition_to_running() {
    let mut j = new_job();
    assert!(j.transition_to(JobState::Parsing));
    assert!(matches!(j.state, JobState::Parsing));
}

#[test]
fn done_blocks_further_transitions() {
    let mut j = new_job();
    j.transition_to(JobState::Parsing);
    j.transition_to(JobState::Done {
        output: OutputPath(PathBuf::from(r"C:\out.mp4")),
        size_mb: 100.0,
    });

    // Stray downloading event arrives after finalize — must be rejected
    let allowed = j.transition_to(JobState::Downloading {
        done: 5,
        total: 10,
        bytes: 1000,
    });
    assert!(
        !allowed,
        "transition_to should return false on terminal state"
    );
    assert!(
        matches!(j.state, JobState::Done { .. }),
        "state must remain Done"
    );
}

#[test]
fn failed_blocks_further_transitions() {
    let mut j = new_job();
    j.transition_to(JobState::Failed {
        error: "boom".into(),
    });
    assert!(!j.transition_to(JobState::Merging {
        ffmpeg_pct: Some(50.0)
    }));
    assert!(matches!(j.state, JobState::Failed { .. }));
}

#[test]
fn is_terminal_only_for_done_and_failed() {
    assert!(!JobState::Queued.is_terminal());
    assert!(!JobState::Parsing.is_terminal());
    assert!(
        !JobState::Downloading {
            done: 0,
            total: 1,
            bytes: 0
        }
        .is_terminal()
    );
    assert!(!JobState::Merging { ffmpeg_pct: None }.is_terminal());
    assert!(
        JobState::Done {
            output: OutputPath(PathBuf::new()),
            size_mb: 0.0
        }
        .is_terminal()
    );
    assert!(
        JobState::Failed {
            error: String::new()
        }
        .is_terminal()
    );
}
