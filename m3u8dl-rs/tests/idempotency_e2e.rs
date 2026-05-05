// file-size-gate: exempt — e2e suite (server spawn boilerplate + 4 tokio::test scenarios
//   exercising idempotency dedupe wiring) follows the existing tests/http_routes.rs and
//   tests/http_sse.rs precedent. Splitting per-test would multiply the spawn_server fixture.

//! End-to-end test of POST /download Idempotency-Key dedupe. Two identical POSTs within
//! TTL → same JobId. Different body → different JobId.

#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use m3u8dl_server::adapters::reqwest_client::ReqwestClient;
use m3u8dl_server::application::download_job::DownloadJob;
use m3u8dl_server::application::idempotency::IdempotencyTable;
use m3u8dl_server::application::job_registry::JobRegistry;
use m3u8dl_server::config::Config;
use m3u8dl_server::http::routes::{AppState, router};
use m3u8dl_server::ports::orchestrator::DownloadOrchestrator;
use serde_json::Value;
use tokio::net::TcpListener;

async fn spawn_server() -> u16 {
    let cfg = Config::default();
    let http = temp_env::with_var("M3U8DL_PROXY", Some("none"), || {
        ReqwestClient::new().expect("client").with_max_retries(0)
    });
    let muxer = m3u8dl_server::adapters::ffmpeg_muxer::FfmpegMuxer::new(
        PathBuf::from("nonexistent-ffmpeg.exe"),
        2.0,
        60,
    );
    let job: Arc<dyn DownloadOrchestrator> = Arc::new(DownloadJob {
        http,
        muxer,
        out_dir: tempfile::tempdir().expect("tmp").keep(),
        parallelism: 4,
    });
    let state = AppState {
        job,
        registry: JobRegistry::new(),
        config: cfg,
        idempotency: IdempotencyTable::new(),
    };
    let app = router(state);
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move { drop(axum::serve(listener, app).await) });
    tokio::time::sleep(Duration::from_millis(50)).await;
    port
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client")
}

const SAMPLE_M3U8: &str =
    "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXTINF:5.0,\nseg-0.ts\n#EXT-X-ENDLIST\n";

#[tokio::test]
async fn same_body_same_idempotency_key_returns_same_job_id() {
    let port = spawn_server().await;
    let body = serde_json::json!({
        "url": "https://cdn.example.com/v/index.m3u8",
        "m3u8": SAMPLE_M3U8,
        "title": "Test",
        "idempotency_key": "client-supplied-key-001"
    });
    let r1: Value = client().post(format!("http://127.0.0.1:{port}/download"))
        .json(&body).send().await.expect("send 1").json().await.expect("json 1");
    let r2: Value = client().post(format!("http://127.0.0.1:{port}/download"))
        .json(&body).send().await.expect("send 2").json().await.expect("json 2");
    assert_eq!(r1["jobId"], r2["jobId"], "expected same JobId for repeat POST");
    assert_eq!(r2["title"], "Test");
}

#[tokio::test]
async fn no_supplied_key_uses_derived_default_for_dedupe() {
    let port = spawn_server().await;
    let body = serde_json::json!({
        "url": "https://cdn.example.com/v/index.m3u8",
        "m3u8": SAMPLE_M3U8,
        "title": "Test"
    });
    let r1: Value = client().post(format!("http://127.0.0.1:{port}/download"))
        .json(&body).send().await.expect("send 1").json().await.expect("json 1");
    let r2: Value = client().post(format!("http://127.0.0.1:{port}/download"))
        .json(&body).send().await.expect("send 2").json().await.expect("json 2");
    assert_eq!(r1["jobId"], r2["jobId"], "derived default key should dedupe");
}

#[tokio::test]
async fn different_url_yields_different_job_id() {
    let port = spawn_server().await;
    let body_a = serde_json::json!({"url": "https://a.example.com/x.m3u8", "m3u8": SAMPLE_M3U8});
    let body_b = serde_json::json!({"url": "https://b.example.com/x.m3u8", "m3u8": SAMPLE_M3U8});
    let r_a: Value = client().post(format!("http://127.0.0.1:{port}/download"))
        .json(&body_a).send().await.expect("send a").json().await.expect("json a");
    let r_b: Value = client().post(format!("http://127.0.0.1:{port}/download"))
        .json(&body_b).send().await.expect("send b").json().await.expect("json b");
    assert_ne!(r_a["jobId"], r_b["jobId"], "different URL should not dedupe");
}

#[tokio::test]
async fn invalid_idempotency_key_returns_400() {
    let port = spawn_server().await;
    let body = serde_json::json!({"m3u8": "#EXTM3U\n", "idempotency_key": "x"});
    let resp = client().post(format!("http://127.0.0.1:{port}/download"))
        .json(&body).send().await.expect("send");
    assert_eq!(resp.status(), 400);
}
