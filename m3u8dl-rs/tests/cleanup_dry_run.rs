//! Verify sweep_old_workdirs dry_run mode: matches stale dirs but doesn't actually delete.

#![allow(clippy::expect_used)]

use std::time::Duration;

use m3u8dl_server::application::cleanup::sweep_old_workdirs;

#[tokio::test]
async fn dry_run_reports_match_without_deleting() {
    let tmp = tempfile::tempdir().expect("tmp");
    let stale_workdir = tmp.path().join("m3u8dl_abc12345");
    tokio::fs::create_dir(&stale_workdir).await.expect("mkdir");
    // Place a file inside so we can verify it stays
    tokio::fs::write(stale_workdir.join("seg-000000.ts"), b"data")
        .await
        .expect("write");

    // Sweep with age_threshold=0 (everything counts as old) + dry_run=true
    let report = sweep_old_workdirs(tmp.path().to_path_buf(), Duration::ZERO, true).await;

    assert_eq!(report.scanned, 1);
    assert_eq!(report.swept, 1, "dry_run should still report swept count");
    assert!(report.dry_run);
    // Directory STILL EXISTS (dry-run = no actual delete)
    assert!(stale_workdir.exists(), "dry_run should not delete");
}

#[tokio::test]
async fn real_sweep_actually_deletes() {
    let tmp = tempfile::tempdir().expect("tmp");
    let stale_workdir = tmp.path().join("m3u8dl_def67890");
    tokio::fs::create_dir(&stale_workdir).await.expect("mkdir");

    let report = sweep_old_workdirs(tmp.path().to_path_buf(), Duration::ZERO, false).await;

    assert_eq!(report.scanned, 1);
    assert_eq!(report.swept, 1);
    assert!(!report.dry_run);
    assert!(!stale_workdir.exists(), "real sweep should delete");
}

#[tokio::test]
async fn skips_non_workdir_named_directories() {
    let tmp = tempfile::tempdir().expect("tmp");
    // not matching the "m3u8dl_*" prefix
    tokio::fs::create_dir(tmp.path().join("random_dir"))
        .await
        .expect("mkdir");
    tokio::fs::create_dir(tmp.path().join("m3u8dl_")) // 7 chars exactly, fails len > 7 check
        .await
        .expect("mkdir");

    let report = sweep_old_workdirs(tmp.path().to_path_buf(), Duration::ZERO, false).await;
    assert_eq!(report.scanned, 0);
    assert_eq!(report.swept, 0);
}

#[tokio::test]
async fn keeps_fresh_directories() {
    let tmp = tempfile::tempdir().expect("tmp");
    let fresh_workdir = tmp.path().join("m3u8dl_fresh001");
    tokio::fs::create_dir(&fresh_workdir).await.expect("mkdir");

    // age_threshold = 1 hour ≫ a freshly-created dir
    let report = sweep_old_workdirs(
        tmp.path().to_path_buf(),
        Duration::from_secs(3600),
        false,
    )
    .await;

    assert_eq!(report.scanned, 1);
    assert_eq!(report.swept, 0, "fresh dir should not be swept");
    assert!(fresh_workdir.exists());
}
