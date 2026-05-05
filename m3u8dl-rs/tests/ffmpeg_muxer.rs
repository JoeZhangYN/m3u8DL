//! Real-ffmpeg integration test. Marked `#[ignore]` because it depends on `ffmpeg.exe` being
//! present at `../ffmpeg.exe` (the project root) and on lavfi's `testsrc` filter.
//! Run with: `cargo test --release --test ffmpeg_muxer -- --ignored`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use m3u8dl_server::adapters::ffmpeg_muxer::FfmpegMuxer;
use m3u8dl_server::domain::{ProgressEvent, SegmentIndex};
use m3u8dl_server::ports::muxer::{Muxer, OrderedSegments};
use m3u8dl_server::ports::progress_sink::ProgressSink;

struct CollectingSink {
    events: Mutex<Vec<ProgressEvent>>,
}
impl CollectingSink {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }
    #[allow(clippy::expect_used)] // helper for tests
    fn snapshot(&self) -> Vec<ProgressEvent> {
        self.events.lock().expect("lock").clone()
    }
}
impl ProgressSink for CollectingSink {
    #[allow(clippy::expect_used)] // helper for tests
    fn emit(&self, event: ProgressEvent) {
        self.events.lock().expect("lock").push(event);
    }
}

fn ffmpeg_path() -> PathBuf {
    // tests run with cwd = m3u8dl-rs/, so ffmpeg.exe is one level up
    PathBuf::from("..").join("ffmpeg.exe")
}

#[allow(clippy::expect_used)] // helper for tests; cmd-spawn failure is fatal anyway
async fn make_test_segment(path: &Path, duration_secs: u32) {
    let ffmpeg = ffmpeg_path();
    let status = tokio::process::Command::new(&ffmpeg)
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=size=320x240:rate=25:duration={duration_secs}"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={duration_secs}"),
            "-c:v",
            "mpeg2video",
            "-c:a",
            "aac",
            "-b:a",
            "64k",
            "-f",
            "mpegts",
        ])
        .arg(path)
        .status()
        .await
        .expect("spawn ffmpeg testsrc");
    assert!(status.success(), "testsrc segment generation failed");
}

#[ignore = "needs ../ffmpeg.exe; run with --ignored"]
#[tokio::test]
async fn muxes_two_test_segments_into_mp4() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let seg0 = tmp.path().join("seg-0.ts");
    let seg1 = tmp.path().join("seg-1.ts");
    let out = tmp.path().join("merged.mp4");

    make_test_segment(&seg0, 1).await;
    make_test_segment(&seg1, 1).await;

    let muxer = FfmpegMuxer::new(ffmpeg_path());
    let sink = CollectingSink::new();
    let inputs =
        OrderedSegments::from_indexed(vec![(SegmentIndex(0), seg0), (SegmentIndex(1), seg1)]);
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        muxer.mux(&inputs, &out, 2.0, &sink),
    )
    .await
    .expect("no hang");
    result.expect("mux ok");

    let meta = std::fs::metadata(&out).expect("output exists");
    assert!(meta.len() >= 1024, "output too small: {} bytes", meta.len());

    let events = sink.snapshot();
    assert!(!events.is_empty(), "expected at least one progress event");
    let last_pct = events.iter().rev().find_map(|e| match e {
        ProgressEvent::Merging { pct } => *pct,
        _ => None,
    });
    assert_eq!(
        last_pct,
        Some(100.0),
        "final event should be 100%, got {last_pct:?}"
    );
}

#[ignore = "needs ../ffmpeg.exe; run with --ignored"]
#[tokio::test]
async fn rejects_empty_input_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("merged.mp4");
    let muxer = FfmpegMuxer::new(ffmpeg_path());
    let sink = CollectingSink::new();
    let inputs = OrderedSegments::from_indexed(vec![]);
    let err = muxer
        .mux(&inputs, &out, 0.0, &sink)
        .await
        .expect_err("should reject");
    assert!(err.to_string().contains("no input"), "got: {err}");
}
