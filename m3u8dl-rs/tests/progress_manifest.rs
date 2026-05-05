//! Round-trip + smart-ctor tests for ProgressManifest.

#![allow(clippy::expect_used)]

use m3u8dl_server::application::progress_manifest::{
    MANIFEST_VERSION, ProgressManifest,
};

#[tokio::test]
async fn round_trip_write_read() {
    let tmp = tempfile::tempdir().expect("tmp");
    let m = ProgressManifest::try_new(
        "job-123".into(),
        Some("https://x.com/y.m3u8".into()),
        vec![5, 1, 3, 2, 0, 4],
        10,
    )
    .expect("ok");
    // Smart-ctor sorts
    assert_eq!(m.segments_downloaded, vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(m.version, MANIFEST_VERSION);

    m.write_to_dir(tmp.path()).await.expect("write");
    let read = ProgressManifest::read_from_dir(tmp.path())
        .await
        .expect("read")
        .expect("Some");

    assert_eq!(read.job_id, m.job_id);
    assert_eq!(read.url, m.url);
    assert_eq!(read.segments_downloaded, m.segments_downloaded);
    assert_eq!(read.total, m.total);
    assert_eq!(read.version, m.version);
}

#[tokio::test]
async fn read_returns_none_when_file_missing() {
    let tmp = tempfile::tempdir().expect("tmp");
    let result = ProgressManifest::read_from_dir(tmp.path()).await.expect("ok");
    assert!(result.is_none());
}

#[tokio::test]
async fn read_returns_none_for_version_mismatch() {
    let tmp = tempfile::tempdir().expect("tmp");
    let bogus_json = r#"{"version": 999, "job_id": "x", "url": null, "segments_downloaded": [], "total": 0, "written_at": "2026-01-01T00:00:00Z"}"#;
    tokio::fs::write(tmp.path().join("progress.json"), bogus_json).await.expect("write");
    let result = ProgressManifest::read_from_dir(tmp.path()).await.expect("ok");
    assert!(result.is_none(), "version mismatch should return None");
}

#[tokio::test]
async fn read_returns_none_for_corrupt_json() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(tmp.path().join("progress.json"), "not valid json {")
        .await
        .expect("write");
    let result = ProgressManifest::read_from_dir(tmp.path()).await.expect("ok");
    assert!(result.is_none(), "corrupt manifest should return None");
}

#[test]
fn try_new_dedups() {
    let m = ProgressManifest::try_new("j".into(), None, vec![1, 1, 2, 2, 3], 5).expect("ok");
    assert_eq!(m.segments_downloaded, vec![1, 2, 3]);
}

#[test]
fn try_new_rejects_len_over_total() {
    let err = ProgressManifest::try_new("j".into(), None, vec![1, 2, 3, 4, 5], 3).expect_err("err");
    assert!(err.to_string().contains("exceeds total"));
}
