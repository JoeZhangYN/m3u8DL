//! Helpers for the `download` route: header allowlist + outbound-header build + spawn task
//! with outer deadline. Split out of routes.rs to keep the router file under the SLOC cap.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::HeaderMap;

use crate::application::download_job::DownloadRequest;
use crate::application::job_registry::JobHandle;
use crate::config::Config;
use crate::domain::{DownloadError, JobId, JobState};
use crate::http::dto::DownloadRequestBody;
use crate::http::routes::AppState;
use crate::ports::progress_sink::ProgressSink;

/// Build the outgoing-fetch headers: defaults (UA / Accept) + client-supplied
/// (Origin / Referer / Cookie). Client wins on conflicts. Allowlist (`is_allowed_header`)
/// drops anything else as defense in depth even though the server only listens on 127.0.0.1.
pub(crate) fn build_outbound_headers(cfg: &Config, body: &DownloadRequestBody) -> HeaderMap {
    let mut headers = cfg.default_headers.clone();
    if let Some(client_headers) = body.headers.as_ref() {
        for (k, v) in client_headers {
            if !is_allowed_header(k) {
                continue;
            }
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                headers.insert(name, value);
            }
        }
    } else if let Some(page) = body.page.as_deref()
        && let Ok(page_url) = url::Url::parse(page)
    {
        if let Ok(origin) =
            reqwest::header::HeaderValue::from_str(&page_url.origin().ascii_serialization())
        {
            headers.insert(reqwest::header::ORIGIN, origin);
        }
        if let Ok(referer) = reqwest::header::HeaderValue::from_str(page) {
            headers.insert(reqwest::header::REFERER, referer);
        }
    }
    headers
}

/// Spawn the background download task with an outer `tokio::time::timeout` of
/// `cfg.job_deadline_secs` (env `M3U8DL_JOB_DEADLINE_SECS`). Three terminal arms:
///   * Ok(Ok((output, size_mb))) → JobState::Done
///   * Ok(Err(e))               → JobState::Failed { e.to_string() }, event=job_failed
///   * Err(_elapsed)            → JobState::Failed (JobDeadlineExceeded), event=job_timeout
pub(crate) fn spawn_download_task(s: &AppState, id: JobId, req: DownloadRequest, handle: JobHandle) {
    let job_runner = s.job.clone();
    let sink_arc: Arc<dyn ProgressSink> = Arc::new(handle.sink.clone());
    let registry = s.registry.clone();
    let deadline = Duration::from_secs(s.config.job_deadline_secs);
    tokio::spawn(async move {
        let final_state = match tokio::time::timeout(deadline, job_runner.run(req, sink_arc)).await
        {
            Ok(Ok((output, size_mb))) => JobState::Done { output, size_mb },
            Ok(Err(e)) => {
                tracing::error!(event = "job_failed", job_id = %id, error = %e);
                JobState::Failed { error: e.to_string() }
            }
            Err(_elapsed) => {
                let secs = deadline.as_secs();
                tracing::error!(event = "job_timeout", job_id = %id, secs);
                JobState::Failed {
                    error: DownloadError::JobDeadlineExceeded { secs }.to_string(),
                }
            }
        };
        registry.set_state(&id, final_state);
    });
}

/// Allowlist of headers that the client may set on upstream fetches.
/// Anything else (e.g. `Authorization`, `Host`, `Content-Length`) is dropped.
pub(crate) fn is_allowed_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "origin" | "referer" | "cookie" | "user-agent" | "accept" | "accept-language" | "x-forwarded-for"
    )
}
