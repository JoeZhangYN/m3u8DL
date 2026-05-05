// Bin entry. Lib (`src/lib.rs`) holds all logic so integration tests in `tests/` can import.

use std::sync::Arc;

use m3u8dl_server::adapters::{ffmpeg_muxer::FfmpegMuxer, reqwest_client::ReqwestClient};
use m3u8dl_server::application::download_job::DownloadJob;
use m3u8dl_server::application::job_registry::JobRegistry;
use m3u8dl_server::config::Config;
use m3u8dl_server::http::routes::{AppState, router};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Subscriber init: text formatter by default (human-readable), JSON when M3U8DL_LOG_FORMAT=json
    // (machine-grep / log aggregation). Both honor RUST_LOG via EnvFilter.
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tower_http=warn"));
    if std::env::var("M3U8DL_LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    let cfg = Config::default();
    let port = cfg.port;
    let out_dir_repr = cfg.out_dir.display().to_string();

    // Fail-fast on out_dir issues — better to refuse to start than fail every download
    if let Err(e) = std::fs::create_dir_all(&cfg.out_dir) {
        tracing::error!(
            event = "startup_aborted",
            rule = "out_dir_unwritable",
            path = %cfg.out_dir.display(),
            error = %e,
            "cannot create output directory; aborting"
        );
        std::process::exit(2);
    }

    let http = ReqwestClient::new()?.with_max_retries(cfg.max_retries);
    let muxer = FfmpegMuxer::new(cfg.ffmpeg_path.clone());
    let job = Arc::new(DownloadJob {
        http,
        muxer,
        out_dir: cfg.out_dir.clone(),
        parallelism: cfg.parallelism,
    });
    let state = AppState {
        job,
        registry: JobRegistry::new(),
        config: cfg,
    };
    let app = router(state);

    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(event = "bind_failed", port, error = %e, "cannot bind TCP listener");
            return Err(e.into());
        }
    };
    // Dual-track: structured event for log aggregation + human-readable banner for terminal users.
    tracing::info!(
        event = "server_started",
        version = env!("CARGO_PKG_VERSION"),
        port,
        out_dir = %out_dir_repr,
        "m3u8dl-server listening"
    );
    println!("=== m3u8dl-server v{} ===", env!("CARGO_PKG_VERSION"));
    println!("listening on http://127.0.0.1:{port}");
    println!("output directory: {out_dir_repr}");
    println!("see docs/SCOPE.md for supported features");
    axum::serve(listener, app).await?;
    Ok(())
}
