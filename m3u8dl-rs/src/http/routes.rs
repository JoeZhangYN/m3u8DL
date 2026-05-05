// error-chain: exempt — HTTP boundary; errors are converted to JSON ErrorBody with status code.

//! axum 0.7 router. Routes mirror `download_server.ps1`:
//! - `GET  /ping`             → `{ ok, port, jobs: count }`
//! - `GET  /status`           → `{ jobs: [snapshot…] }`
//! - `POST /download`         → 202 `{ jobId, status:"queued", title }`
//! - `GET  /job/:id`          → snapshot or 404
//!
//! CORS: `Access-Control-Allow-Origin: *` (server only listens on 127.0.0.1).

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::cors::{Any, CorsLayer};

use crate::adapters::ffmpeg_muxer::FfmpegMuxer;
use crate::adapters::reqwest_client::ReqwestClient;
use crate::application::download_job::{DownloadJob, DownloadRequest};
use crate::application::job_registry::JobRegistry;
use crate::config::Config;
use crate::domain::{JobId, JobState, M3u8Input};
use crate::http::dto::{
    DownloadAccepted, DownloadRequestBody, ErrorBody, JobSnapshot, PingResponse, StatusResponse,
};
use crate::ports::progress_sink::ProgressSink;

pub type Job = DownloadJob<ReqwestClient, FfmpegMuxer>;

#[derive(Clone)]
pub struct AppState {
    pub job: Arc<Job>,
    pub registry: JobRegistry,
    pub config: Config,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ping", get(ping))
        .route("/status", get(status))
        .route("/download", post(download))
        .route("/job/:id", get(get_job))
        .route("/events/:id", get(crate::http::sse::job_events))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

async fn ping(State(s): State<AppState>) -> Json<PingResponse> {
    Json(PingResponse {
        ok: true,
        port: s.config.port,
        jobs: s.registry.count(),
    })
}

async fn status(State(s): State<AppState>) -> Json<StatusResponse> {
    let mut snapshots = Vec::new();
    for id in s.registry.ids() {
        if let Some(h) = s.registry.get(&id)
            && let Ok(g) = h.job.lock()
        {
            snapshots.push(JobSnapshot::from_job(&g));
        }
    }
    Json(StatusResponse { jobs: snapshots })
}

#[allow(clippy::expect_used)] // poisoned mutex = bug, panic to surface it
async fn get_job(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<JobSnapshot>, (StatusCode, Json<ErrorBody>)> {
    let job_id = JobId::from_string(id);
    let h = s.registry.get(&job_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: format!("job {job_id} not found"),
            }),
        )
    })?;
    let g = h.job.lock().expect("job mutex poisoned");
    Ok(Json(JobSnapshot::from_job(&g)))
}

async fn download(
    State(s): State<AppState>,
    Json(body): Json<DownloadRequestBody>,
) -> Result<(StatusCode, Json<DownloadAccepted>), (StatusCode, Json<ErrorBody>)> {
    let id = JobId::new();
    let title = body.title.unwrap_or_else(|| format!("video_{id}"));
    let source_url = body.url.as_deref().and_then(|u| url::Url::parse(u).ok());
    // `M3u8Input::detect` is the single point of input classification (incl. empty check).
    let input = match M3u8Input::detect(&body.m3u8) {
        Ok(i) => i,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            ));
        }
    };
    let handle = s.registry.register(id.clone(), title.clone());

    // Build the outgoing-fetch headers: defaults (UA / Accept) + client-supplied
    // (Origin / Referer / Cookie). Client wins on conflicts. Allowlist: only forward
    // headers we know are safe for upstream HLS fetches — defense in depth even though
    // the server only listens on 127.0.0.1.
    let mut headers = s.config.default_headers.clone();
    if let Some(client_headers) = body.headers {
        for (k, v) in client_headers {
            if !is_allowed_header(&k) {
                continue;
            }
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(&v),
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

    let req = DownloadRequest {
        input,
        source_url,
        title: title.clone(),
        headers,
    };
    let job_runner = s.job.clone();
    let sink_arc: Arc<dyn ProgressSink> = Arc::new(handle.sink.clone());
    let registry = s.registry.clone();
    let id_for_task = id.clone();
    tokio::spawn(async move {
        let final_state = match job_runner.run(req, sink_arc).await {
            Ok((output, size_mb)) => JobState::Done { output, size_mb },
            Err(e) => {
                tracing::error!(job_id = %id_for_task, error = %e, "download job failed");
                JobState::Failed {
                    error: e.to_string(),
                }
            }
        };
        registry.set_state(&id_for_task, final_state);
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(DownloadAccepted {
            job_id: id.as_str().to_string(),
            status: "queued",
            title,
        }),
    ))
}

/// Allowlist of headers that the client may set on upstream fetches.
/// Anything else (e.g. `Authorization`, `Host`, `Content-Length`) is dropped.
fn is_allowed_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "origin"
            | "referer"
            | "cookie"
            | "user-agent"
            | "accept"
            | "accept-language"
            | "x-forwarded-for"
    )
}
