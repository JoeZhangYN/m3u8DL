//! SSE endpoint test: POST /download, then GET /job/{id}/events and verify the snapshot
//! event arrives. Background job will fail (no ffmpeg), which actually exercises the
//! "Failed" event path in the broadcast stream.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use m3u8dl_server::adapters::ffmpeg_muxer::FfmpegMuxer;
use m3u8dl_server::adapters::reqwest_client::ReqwestClient;
use m3u8dl_server::application::download_job::DownloadJob;
use m3u8dl_server::application::job_registry::JobRegistry;
use m3u8dl_server::config::Config;
use m3u8dl_server::http::routes::{AppState, router};
use m3u8dl_server::ports::orchestrator::DownloadOrchestrator;
use serde_json::Value;
use tokio::net::TcpListener;

#[allow(clippy::expect_used)] // tests/* helper
async fn spawn_server(out_dir: PathBuf) -> u16 {
    let cfg = Config::default();
    let http = ReqwestClient::new().expect("client").with_max_retries(0);
    let muxer = FfmpegMuxer::new(PathBuf::from("nonexistent-ffmpeg.exe"), 2.0, 60);
    let job: Arc<dyn DownloadOrchestrator> = Arc::new(DownloadJob {
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
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        drop(axum::serve(listener, app).await);
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    port
}

#[tokio::test]
async fn sse_404_for_unknown_job() {
    let tmp = tempfile::tempdir().expect("tmp");
    let port = spawn_server(tmp.path().to_path_buf()).await;
    let resp = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .http1_only()
        .build()
        .expect("client")
        .get(format!("http://127.0.0.1:{port}/events/notarealjob"))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn sse_streams_initial_snapshot_event() {
    let tmp = tempfile::tempdir().expect("tmp");
    let port = spawn_server(tmp.path().to_path_buf()).await;
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .http1_only()
        .build()
        .expect("client");

    let post: Value = client
        .post(format!("http://127.0.0.1:{port}/download"))
        .json(&serde_json::json!({
            "m3u8": "#EXTM3U\n#EXTINF:1.0,\nhttp://nope.invalid/seg.ts\n#EXT-X-ENDLIST\n",
            "title": "sse_test"
        }))
        .send()
        .await
        .expect("post")
        .json()
        .await
        .expect("json");
    let job_id = post["jobId"].as_str().expect("jobId").to_string();

    // Open SSE stream and read the first few bytes — must contain the snapshot event
    let resp = client
        .get(format!("http://127.0.0.1:{port}/events/{job_id}"))
        .send()
        .await
        .expect("sse send");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let read = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.expect("chunk");
            buf.push_str(&String::from_utf8_lossy(&bytes));
            if buf.contains("\n\n") {
                break;
            }
        }
    })
    .await;
    assert!(
        read.is_ok(),
        "timed out waiting for first SSE event; buf so far: {buf:?}"
    );
    assert!(
        buf.contains("event: snapshot"),
        "expected snapshot event, got: {buf:?}"
    );
    assert!(
        buf.contains("\"id\":"),
        "snapshot data missing 'id': {buf:?}"
    );
    assert!(
        buf.contains("\"title\":\"sse_test\""),
        "snapshot title mismatch: {buf:?}"
    );
}
