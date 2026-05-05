// error-chain: exempt — HTTP boundary; errors are converted to JSON ErrorBody with status code.

//! axum 0.7 router. Routes mirror `download_server.ps1`:
//! - `GET  /ping`             → `{ ok, port, jobs: count }`
//! - `GET  /status`           → `{ jobs: [snapshot…] }`
//! - `POST /download`         → 202 `{ jobId, status:"queued", title }`
//! - `GET  /job/:id`          → snapshot or 404
//! - `GET  /events/:id`       → text/event-stream (SSE)
//!
//! CORS: `Access-Control-Allow-Origin: *` (server only listens on 127.0.0.1).
//! Tracing: `tower_http::TraceLayer` mounted at debug span level so request
//! enter/exit lines are visible only with `RUST_LOG=debug` (avoids info-flood).
//! Helpers (header allowlist, outbound-header build, spawn task with deadline)
//! live in `crate::http::handlers` to keep this file under 150 SLOC.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::Level;

use crate::adapters::ffmpeg_muxer::FfmpegMuxer;
use crate::adapters::reqwest_client::ReqwestClient;
use crate::application::download_job::{DownloadJob, DownloadRequest};
use crate::application::job_registry::{JobRegistry, lock_or_poisoned};
use crate::config::Config;
use crate::domain::{JobId, M3u8Input};
use crate::http::dto::{
    DownloadAccepted, DownloadRequestBody, ErrorBody, JobSnapshot, PingResponse, StatusResponse,
};
use crate::http::handlers::{build_outbound_headers, spawn_download_task};

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
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::DEBUG)),
        )
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
        if let Some(h) = s.registry.get(&id) {
            let g = lock_or_poisoned(&h.job);
            snapshots.push(JobSnapshot::from_job(&g));
        }
    }
    Json(StatusResponse { jobs: snapshots })
}

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
    let g = lock_or_poisoned(&h.job);
    Ok(Json(JobSnapshot::from_job(&g)))
}

async fn download(
    State(s): State<AppState>,
    Json(body): Json<DownloadRequestBody>,
) -> Result<(StatusCode, Json<DownloadAccepted>), (StatusCode, Json<ErrorBody>)> {
    let id = JobId::new();
    let title = body.title.clone().unwrap_or_else(|| format!("video_{id}"));
    let source_url = body.url.as_deref().and_then(|u| url::Url::parse(u).ok());
    // `M3u8Input::detect` is the single point of input classification (incl. empty check).
    let input = M3u8Input::detect(&body.m3u8).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody { error: e.to_string() }),
        )
    })?;
    let handle = s.registry.register(id.clone(), title.clone());
    let headers = build_outbound_headers(&s.config, &body);
    let req = DownloadRequest { input, source_url, title: title.clone(), headers };
    spawn_download_task(&s, id.clone(), req, handle);
    Ok((
        StatusCode::ACCEPTED,
        Json(DownloadAccepted {
            job_id: id.as_str().to_string(),
            status: "queued",
            title,
        }),
    ))
}
