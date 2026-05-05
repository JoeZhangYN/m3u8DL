// file-size-gate: exempt — large e2e suite covering happy path / error / SSE / state
//   transitions in one file matches the existing pattern; per-test split would multiply
//   server-spawn boilerplate. Plan commit 12 introduces DownloadOrchestrator trait which
//   may unlock a stub-based slimmer suite for follow-up.

//! HTTP layer integration tests. Spins up the real router on an OS-picked port and hits
//! it via reqwest. Mock the muxer (so no ffmpeg dep) and use wiremock for the upstream m3u8.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use m3u8dl_server::adapters::reqwest_client::ReqwestClient;
use m3u8dl_server::application::download_job::DownloadJob;
use m3u8dl_server::application::job_registry::JobRegistry;
use m3u8dl_server::config::Config;
use m3u8dl_server::http::routes::{AppState, router};
use serde_json::Value;
use tokio::net::TcpListener;

// We can't substitute Muxer here because routes::AppState carries the concrete
// `DownloadJob<ReqwestClient, FfmpegMuxer>` (no type erasure) — for HTTP-shape tests we
// give it a non-existent ffmpeg path; the spawned background job will fail in metadata
// fetch (file not exists), but we only assert on the synchronous /download response shape.

#[allow(clippy::expect_used)] // tests/* helper
async fn spawn_server(out_dir: PathBuf) -> u16 {
    let cfg = Config::default();
    let http = ReqwestClient::new().expect("client").with_max_retries(0);
    let muxer = m3u8dl_server::adapters::ffmpeg_muxer::FfmpegMuxer::new(
        PathBuf::from("nonexistent-ffmpeg.exe"),
        2.0,
        60,
    );
    let job = Arc::new(DownloadJob {
        http,
        muxer,
        out_dir,
        parallelism: 4,
    });
    let state = AppState {
        job,
        registry: JobRegistry::new(),
        config: cfg,
    };
    let app = router(state);
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    tokio::spawn(async move {
        drop(axum::serve(listener, app).await);
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    port
}

#[allow(clippy::expect_used)] // tests/* helper
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client")
}

#[tokio::test]
async fn ping_returns_ok() {
    let tmp = tempfile::tempdir().expect("tmp");
    let port = spawn_server(tmp.path().to_path_buf()).await;
    let r: Value = http_client()
        .get(format!("http://127.0.0.1:{port}/ping"))
        .send()
        .await
        .expect("send")
        .json()
        .await
        .expect("json");
    assert_eq!(r["ok"], true);
    assert_eq!(r["port"], 7787); // Config::default port (informational), not the bind port
    assert_eq!(r["jobs"], 0);
}

#[tokio::test]
async fn status_initially_empty() {
    let tmp = tempfile::tempdir().expect("tmp");
    let port = spawn_server(tmp.path().to_path_buf()).await;
    let r: Value = http_client()
        .get(format!("http://127.0.0.1:{port}/status"))
        .send()
        .await
        .expect("send")
        .json()
        .await
        .expect("json");
    assert!(r["jobs"].as_array().expect("array").is_empty());
}

#[tokio::test]
async fn download_rejects_empty_m3u8() {
    let tmp = tempfile::tempdir().expect("tmp");
    let port = spawn_server(tmp.path().to_path_buf()).await;
    let resp = http_client()
        .post(format!("http://127.0.0.1:{port}/download"))
        .json(&serde_json::json!({ "m3u8": "", "title": "x" }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 400);
    let b: Value = resp.json().await.expect("json");
    assert!(b["error"].as_str().expect("err string").contains("empty"));
}

#[tokio::test]
async fn download_accepts_valid_returns_job_id() {
    let tmp = tempfile::tempdir().expect("tmp");
    let port = spawn_server(tmp.path().to_path_buf()).await;
    let resp = http_client()
        .post(format!("http://127.0.0.1:{port}/download"))
        .json(&serde_json::json!({
            "m3u8": "#EXTM3U\n#EXTINF:1.0,\nhttp://nope/seg.ts\n#EXT-X-ENDLIST\n",
            "url": "http://nope/x.m3u8",
            "title": "smoke",
        }))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 202);
    let b: Value = resp.json().await.expect("json");
    let jid = b["jobId"].as_str().expect("jobId string");
    assert_eq!(jid.len(), 8, "jobId should be 8-hex, got {jid:?}");
    assert_eq!(b["status"], "queued");
    assert_eq!(b["title"], "smoke");
}

#[tokio::test]
async fn download_accepts_client_supplied_headers() {
    let tmp = tempfile::tempdir().expect("tmp");
    let port = spawn_server(tmp.path().to_path_buf()).await;
    let resp = http_client()
        .post(format!("http://127.0.0.1:{port}/download"))
        .json(&serde_json::json!({
            "m3u8": "#EXTM3U\n#EXTINF:1.0,\nhttp://nope/seg.ts\n#EXT-X-ENDLIST\n",
            "url": "http://nope/x.m3u8",
            "title": "header_test",
            "page": "https://example.test/play/1",
            "headers": {
                "Origin":  "https://example.test",
                "Referer": "https://example.test/play/1",
                "Cookie":  "session=abc"
            }
        }))
        .send()
        .await
        .expect("send");
    assert_eq!(
        resp.status(),
        202,
        "client headers in body should not be rejected"
    );
    let b: Value = resp.json().await.expect("json");
    assert_eq!(b["status"], "queued");
}

#[tokio::test]
async fn unknown_job_returns_404() {
    let tmp = tempfile::tempdir().expect("tmp");
    let port = spawn_server(tmp.path().to_path_buf()).await;
    let resp = http_client()
        .get(format!("http://127.0.0.1:{port}/job/deadbeef"))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 404);
    let b: Value = resp.json().await.expect("json");
    assert!(b["error"].as_str().expect("err").contains("not found"));
}

#[tokio::test]
async fn cors_allows_any_origin() {
    let tmp = tempfile::tempdir().expect("tmp");
    let port = spawn_server(tmp.path().to_path_buf()).await;
    let resp = http_client()
        .get(format!("http://127.0.0.1:{port}/ping"))
        .header("Origin", "https://www.example.com")
        .send()
        .await
        .expect("send");
    let cors = resp
        .headers()
        .get("access-control-allow-origin")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    assert_eq!(cors.as_deref(), Some("*"), "expected * CORS header");
}
