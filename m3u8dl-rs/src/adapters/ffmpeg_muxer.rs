// error-chain: exempt — DownloadError::Ffmpeg variant already carries `code` + `stderr` (full
// ffmpeg log on failure). Wrapping every `?` with path context would only duplicate info
// already present in the structured Ffmpeg variant or the surrounding fn signature.

//! ffmpeg-based muxer: spawn external `ffmpeg.exe -f concat -i list.txt -c copy out.mp4`,
//! parse `-progress pipe:2` key=value lines from stderr to drive the merge progress bar.
//!
//! Output container: mpeg-4 with `aac_adtstoasc` bitstream filter (matches the legacy
//! `pipeline.ps1` final ffmpeg invocation). No re-encoding — pure stream copy.
//!
//! Deadline: mux is bounded by `total_duration_secs * factor + min_secs`. Stream copy
//! should be fast (≪ playback duration); on timeout the child is killed and the call
//! returns `DownloadError::FfmpegTimeout` (audit-resilience.timeout-missing FAIL).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStderr, Command};

use crate::domain::{DownloadError, ProgressEvent, Result};
use crate::ports::muxer::{Muxer, OrderedSegments};
use crate::ports::progress_sink::ProgressSink;

#[derive(Clone)]
pub struct FfmpegMuxer {
    ffmpeg_path: PathBuf,
    deadline_factor: f64,
    deadline_min_secs: u64,
}

impl FfmpegMuxer {
    pub fn new(ffmpeg_path: PathBuf, deadline_factor: f64, deadline_min_secs: u64) -> Self {
        Self {
            ffmpeg_path,
            deadline_factor,
            deadline_min_secs,
        }
    }
}

impl Muxer for FfmpegMuxer {
    async fn mux(
        &self,
        inputs: &OrderedSegments,
        output: &Path,
        total_duration_secs: f64,
        sink: &dyn ProgressSink,
    ) -> Result<()> {
        if inputs.is_empty() {
            return Err(ffmpeg_err("no input segments"));
        }
        let work_dir = output.parent().ok_or_else(|| ffmpeg_err("output has no parent dir"))?;
        tokio::fs::create_dir_all(work_dir).await?;
        let concat_path = work_dir.join("concat.txt");
        write_concat_list(&concat_path, inputs.paths()).await?;

        let mut child = Command::new(&self.ffmpeg_path)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i"])
            .arg(&concat_path)
            .args(["-c", "copy", "-bsf:a", "aac_adtstoasc", "-progress", "pipe:2"])
            .arg(output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ffmpeg_err(&format!("spawn ffmpeg: {e}")))?;

        let stderr = child.stderr.take().ok_or_else(|| ffmpeg_err("stderr pipe missing"))?;
        let deadline = Duration::from_secs(
            ((total_duration_secs.max(0.0) * self.deadline_factor) as u64)
                .saturating_add(self.deadline_min_secs),
        );

        // Run stderr-read loop + child.wait() under a single timeout. On elapse, kill the
        // child to release the OS process and any held FDs (audit-resilience.timeout-missing).
        let work = async {
            let lines = read_progress_until_eof(stderr, sink, total_duration_secs).await?;
            let status = child.wait().await?;
            Ok::<_, DownloadError>((status, lines))
        };
        let (status, error_lines) = match tokio::time::timeout(deadline, work).await {
            Ok(r) => r?,
            Err(_) => {
                drop(child.kill().await);
                return Err(DownloadError::FfmpegTimeout { secs: deadline.as_secs() });
            }
        };

        if !status.success() {
            let stderr_msg = if error_lines.is_empty() { "(no stderr)".into() } else { error_lines.join("\n") };
            return Err(DownloadError::Ffmpeg {
                code: status.code().unwrap_or(-1),
                stderr: stderr_msg,
            });
        }
        sink.emit(ProgressEvent::merging(Some(100.0)));
        Ok(())
    }
}

/// Read ffmpeg `-progress pipe:2` key=value lines until EOF; emit merge percentage events
/// (1% threshold debounce) and collect non-key=value lines for the failure stderr message.
async fn read_progress_until_eof(
    stderr: ChildStderr,
    sink: &dyn ProgressSink,
    total_duration_secs: f64,
) -> Result<Vec<String>> {
    let mut error_lines: Vec<String> = Vec::new();
    let mut last_pct = 0.0_f32;
    let mut reader = BufReader::new(stderr).lines();
    while let Some(line) = reader.next_line().await? {
        if let Some(raw) = line.strip_prefix("out_time_ms=")
            && let Ok(us) = raw.trim().parse::<u64>()
            && total_duration_secs > 0.0
        {
            let secs = us as f64 / 1_000_000.0;
            let pct = ((secs / total_duration_secs * 100.0) as f32).clamp(0.0, 100.0);
            if (pct - last_pct).abs() >= 1.0 {
                sink.emit(ProgressEvent::merging(Some(pct)));
                last_pct = pct;
            }
        } else if !line.contains('=') && !line.is_empty() {
            error_lines.push(line);
        }
    }
    Ok(error_lines)
}

fn ffmpeg_err(msg: &str) -> DownloadError {
    DownloadError::Ffmpeg { code: -1, stderr: msg.into() }
}

async fn write_concat_list(path: &Path, inputs: &[PathBuf]) -> Result<()> {
    let mut text = String::with_capacity(inputs.len() * 64);
    for p in inputs {
        let escaped = p.display().to_string().replace('\'', r"'\''");
        text.push_str(&format!("file '{escaped}'\n"));
    }
    tokio::fs::write(path, text).await?;
    Ok(())
}
