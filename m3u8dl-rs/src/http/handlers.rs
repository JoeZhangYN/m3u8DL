//! Helpers for the `download` route: header allowlist + outbound-header build + spawn task
//! with outer deadline. Split out of routes.rs to keep the router file under the SLOC cap.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use axum::Json;
use reqwest::header::HeaderMap;

use crate::application::download_job::DownloadRequest;
use crate::application::idempotency::{IdempotencyEntry, IdempotencyKey, IdempotencyKeyError};
use crate::application::job_registry::JobHandle;
use crate::config::Config;
use crate::domain::{DownloadError, JobId, JobState};
use crate::http::dto::{DownloadAccepted, DownloadRequestBody, ErrorBody};
use crate::http::routes::AppState;
use crate::ports::progress_sink::ProgressSink;

/// Resolve an idempotency key from request body: if client supplied one, validate it;
/// otherwise derive the default `sha256(url + sorted_headers)` key. Returns either a
/// validated `IdempotencyKey` or a 400 response payload.
pub(crate) fn resolve_idempotency_key(
    body: &DownloadRequestBody,
) -> Result<IdempotencyKey, (StatusCode, Json<ErrorBody>)> {
    match body.idempotency_key.as_deref() {
        Some(supplied) => IdempotencyKey::try_from(supplied).map_err(|e: IdempotencyKeyError| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody { error: e.to_string() }),
            )
        }),
        None => {
            let url = body.url.as_deref().unwrap_or("");
            let empty = std::collections::HashMap::new();
            let headers = body.headers.as_ref().unwrap_or(&empty);
            Ok(IdempotencyKey::derive_default(url, headers))
        }
    }
}

/// Look up `key` in the table; if a fresh entry exists, return the cached 202 response.
/// `None` → caller proceeds with normal job creation.
pub(crate) fn try_idempotent_response(
    s: &AppState,
    key: &IdempotencyKey,
) -> Option<(StatusCode, Json<DownloadAccepted>)> {
    let ttl = Duration::from_secs(s.config.idempotency_ttl_secs);
    let entry = s.idempotency.lookup_fresh(key, Instant::now(), ttl)?;
    tracing::info!(
        event = "idempotent_hit",
        job_id = %entry.job_id,
        key = key.as_str(),
        "returning cached jobId for repeat POST"
    );
    Some((
        StatusCode::ACCEPTED,
        Json(DownloadAccepted {
            job_id: entry.job_id.as_str().to_string(),
            status: "queued",
            title: entry.title,
        }),
    ))
}

/// Persist a fresh job's idempotency mapping so subsequent identical POSTs hit
/// `try_idempotent_response`.
pub(crate) fn record_idempotent(s: &AppState, key: IdempotencyKey, job_id: JobId, title: String) {
    s.idempotency
        .insert(key, IdempotencyEntry::new(job_id, title));
}

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

#[cfg(test)]
mod tests {
    use super::*;

    // Allow list — 7 canonical lowercase
    #[test]
    fn allows_all_seven_canonical() {
        for h in ["origin", "referer", "cookie", "user-agent", "accept", "accept-language", "x-forwarded-for"] {
            assert!(is_allowed_header(h), "expected allow: {h}");
        }
    }

    // Case-insensitive
    #[test]
    fn allows_mixed_case() {
        assert!(is_allowed_header("ORIGIN"));
        assert!(is_allowed_header("Cookie"));
        assert!(is_allowed_header("User-Agent"));
        assert!(is_allowed_header("Accept-Language"));
        assert!(is_allowed_header("USER-AGENT"));
    }

    // Reject sensitive headers (regression vectors)
    #[test]
    fn rejects_authorization() {
        for h in ["authorization", "Authorization", "AUTHORIZATION"] {
            assert!(!is_allowed_header(h), "expected reject: {h}");
        }
    }

    #[test]
    fn rejects_host_and_content_headers() {
        for h in ["host", "Host", "content-length", "Content-Length", "content-type", "Content-Type"] {
            assert!(!is_allowed_header(h), "expected reject: {h}");
        }
    }

    #[test]
    fn rejects_arbitrary_x_headers() {
        for h in ["x-custom", "x-api-key", "X-Forwarded-Host", "x-real-ip"] {
            assert!(!is_allowed_header(h), "expected reject: {h}");
        }
    }

    #[test]
    fn rejects_proxy_authorization() {
        assert!(!is_allowed_header("proxy-authorization"));
        assert!(!is_allowed_header("Proxy-Authorization"));
    }

    // Whitespace edges — must NOT match (callers feed canonical names)
    #[test]
    fn rejects_leading_or_trailing_whitespace() {
        for h in [" origin", "origin ", "\torigin", "origin\t"] {
            assert!(!is_allowed_header(h), "expected reject: {h:?}");
        }
    }

    #[test]
    fn rejects_internal_space() {
        assert!(!is_allowed_header("user agent"));
    }

    // Edge / partial match
    #[test]
    fn rejects_empty() {
        assert!(!is_allowed_header(""));
    }

    #[test]
    fn rejects_partial_match() {
        // prefix / suffix / substring of allowlisted name must not match
        for h in ["or", "origin-extended", "super-origin", "userref"] {
            assert!(!is_allowed_header(h), "expected reject: {h}");
        }
    }
}
