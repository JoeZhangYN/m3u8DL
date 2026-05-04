// Bin entry. Lib (`src/lib.rs`) holds all logic so integration tests in `tests/` can import.

use std::sync::Arc;

use m3u8dl_server::adapters::{ffmpeg_muxer::FfmpegMuxer, reqwest_client::ReqwestClient};
use m3u8dl_server::application::download_job::DownloadJob;
use m3u8dl_server::application::job_registry::JobRegistry;
use m3u8dl_server::config::Config;
use m3u8dl_server::http::routes::{AppState, router};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tower_http=warn")),
        )
        .init();

    let cfg = Config::default();
    let port = cfg.port;
    let out_dir_repr = cfg.out_dir.display().to_string();

    // Fail-fast on out_dir issues — better to refuse to start than fail every download
    if let Err(e) = std::fs::create_dir_all(&cfg.out_dir) {
        eprintln!("FATAL: cannot create output directory {}: {e}", cfg.out_dir.display());
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
    let state = AppState { job, registry: JobRegistry::new(), config: cfg };
    let app = router(state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("=== m3u8dl-server v{} ===", env!("CARGO_PKG_VERSION"));
    println!("listening on http://127.0.0.1:{port}");
    println!("output directory: {out_dir_repr}");
    println!("see docs/SCOPE.md for supported features");
    axum::serve(listener, app).await?;
    Ok(())
}
