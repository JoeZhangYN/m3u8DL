//! Periodic cleanup of stale per-job temp directories. Failed jobs deliberately leave
//! `m3u8dl_<id>/` in `%TEMP%` for post-mortem (Tier B audit-idempotency.temp-leak fix kept
//! these intentionally on failure paths). Without periodic sweep, those accumulate
//! across server restarts. This module sweeps directories older than `age_threshold`.
//!
//! Conservative: re-stats mtime in the sweep loop (catches a job that started recently
//! but predates the scan); supports `dry_run=true` for ops audit before enabling deletes.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::application::idempotency::IdempotencyTable;

/// Result of a sweep pass — emitted as a tracing event so ops can audit the impact.
#[derive(Debug, Clone, Default)]
pub struct SweepReport {
    pub scanned: usize,
    pub swept: usize,
    pub errors: usize,
    pub dry_run: bool,
}

/// Sweep `<temp_root>/m3u8dl_*/` directories older than `age_threshold`. IO errors are
/// logged + counted but do NOT abort the sweep — best-effort cleanup.
pub async fn sweep_old_workdirs(
    temp_root: PathBuf,
    age_threshold: Duration,
    dry_run: bool,
) -> SweepReport {
    let mut report = SweepReport {
        dry_run,
        ..Default::default()
    };
    let mut dir = match tokio::fs::read_dir(&temp_root).await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(
                event = "sweep_read_dir_failed",
                path = %temp_root.display(),
                error = %e
            );
            report.errors = 1;
            return report;
        }
    };
    let now = SystemTime::now();
    loop {
        match dir.next_entry().await {
            Ok(Some(entry)) => {
                let path = entry.path();
                if !is_workdir_candidate(&path) {
                    continue;
                }
                report.scanned += 1;
                let mtime = match entry.metadata().await.and_then(|m| m.modified()) {
                    Ok(t) => t,
                    Err(_) => {
                        report.errors += 1;
                        continue;
                    }
                };
                let age = match now.duration_since(mtime) {
                    Ok(a) => a,
                    Err(_) => Duration::ZERO, // mtime in the future → treat as fresh
                };
                if age < age_threshold {
                    continue; // too fresh; an active or recent job
                }
                if dry_run {
                    tracing::info!(
                        event = "sweep_would_delete",
                        path = %path.display(),
                        age_secs = age.as_secs()
                    );
                    report.swept += 1;
                } else if let Err(e) = tokio::fs::remove_dir_all(&path).await {
                    tracing::warn!(
                        event = "sweep_remove_failed",
                        path = %path.display(),
                        error = %e
                    );
                    report.errors += 1;
                } else {
                    tracing::info!(event = "sweep_deleted", path = %path.display(), age_secs = age.as_secs());
                    report.swept += 1;
                }
            }
            Ok(None) => break,
            Err(e) => {
                tracing::warn!(event = "sweep_iter_failed", error = %e);
                report.errors += 1;
                break;
            }
        }
    }
    report
}

/// Match the `m3u8dl_<hex>` shape that `make_temp_workdir` creates. Conservative — won't
/// touch a directory that doesn't fit the prefix.
fn is_workdir_candidate(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("m3u8dl_") && n.len() > 7)
}

/// Background loop: every `interval`, sweep both the temp_root AND the IdempotencyTable's
/// expired entries. main.rs spawns this once at startup and forgets it.
pub async fn periodic_sweep(
    temp_root: PathBuf,
    age_threshold: Duration,
    interval: Duration,
    idempotency_ttl: Duration,
    dry_run: bool,
    idempotency: IdempotencyTable,
) {
    let mut ticker = tokio::time::interval(interval);
    // Skip the immediate-fire tick — let the server come up before we start sweeping.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        let report = sweep_old_workdirs(temp_root.clone(), age_threshold, dry_run).await;
        if report.scanned > 0 || report.errors > 0 {
            tracing::info!(
                event = "sweep_workdirs_complete",
                scanned = report.scanned,
                swept = report.swept,
                errors = report.errors,
                dry_run = report.dry_run,
            );
        }
        let removed = idempotency.sweep_expired(std::time::Instant::now(), idempotency_ttl);
        if removed > 0 {
            tracing::info!(event = "sweep_idempotency_complete", removed);
        }
    }
}
