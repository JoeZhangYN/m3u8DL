// error-chain: exempt — IO/path errors here are wrapped in DownloadError variants that
// already carry path/operation context; orchestrator-level failures are logged via tracing.

//! Download orchestrator. Wires: input detection → playlist fetch → PNG strip / normalize →
//! parse → parallel segment fetch+decrypt → ffmpeg mux → output validation. Generic over
//! `HttpClient` and `Muxer` so tests inject wiremock + a stub muxer.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use futures_util::{StreamExt, TryStreamExt, stream};
use reqwest::header::HeaderMap;
use url::Url;

use crate::adapters::{m3u8_normalize, png_strip};
use crate::application::base_url::derive_base_url;
use crate::application::parser::parse_m3u8;
use crate::application::segment_fetcher::{fetch_one, new_key_cache};
use crate::domain::{
    DownloadError, M3u8Input, MediaPlaylist, OutputPath, Playlist, ProgressEvent, Result, Variant,
};
use crate::ports::http_client::{HttpClient, HttpRequest};
use crate::ports::muxer::{Muxer, OrderedSegments};
use crate::ports::progress_sink::ProgressSink;

pub struct DownloadJob<C, M> {
    pub http: C,
    pub muxer: M,
    pub out_dir: PathBuf,
    pub parallelism: usize,
}

pub struct DownloadRequest {
    pub input: M3u8Input,
    pub source_url: Option<Url>,
    pub title: String,
    pub headers: HeaderMap,
}

impl<C: HttpClient + 'static, M: Muxer + 'static> DownloadJob<C, M> {
    pub async fn run(
        &self,
        req: DownloadRequest,
        sink: Arc<dyn ProgressSink>,
    ) -> Result<(OutputPath, f64)> {
        sink.emit(ProgressEvent::Parsing);
        let media = self.resolve_media(&req).await?;
        let work_dir = make_temp_workdir().await?;

        let total = media.segments.len() as u32;
        let total_dur = media.total_duration;
        let counter = Arc::new(AtomicU32::new(0));
        let bytes_total = Arc::new(AtomicU64::new(0));
        let key_cache = new_key_cache();

        let collected: Vec<_> = stream::iter(media.segments)
            .map(|seg| {
                let http = &self.http;
                let key_cache = key_cache.clone();
                let counter = counter.clone();
                let bytes_total = bytes_total.clone();
                let work_dir = work_dir.clone();
                let sink = sink.clone();
                let headers = req.headers.clone();
                async move {
                    let f = fetch_one(seg, http, &key_cache, &headers, &work_dir).await?;
                    let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    let total_b =
                        bytes_total.fetch_add(f.bytes_written, Ordering::SeqCst) + f.bytes_written;
                    sink.emit(ProgressEvent::downloading(n, total, total_b, f.idx.0));
                    Ok::<_, DownloadError>((f.idx, f.path))
                }
            })
            .buffer_unordered(self.parallelism)
            .try_collect()
            .await?;
        // OrderedSegments::from_indexed sorts by SegmentIndex — muxer is now type-guaranteed
        // to receive segments in playlist order without trusting the caller.
        let inputs = OrderedSegments::from_indexed(collected);

        sink.emit(ProgressEvent::merging(Some(0.0)));
        let output = self.out_dir.join(format!("{}.mp4", sanitize(&req.title)));
        tokio::fs::create_dir_all(&self.out_dir).await?;
        self.muxer
            .mux(&inputs, &output, total_dur, sink.as_ref())
            .await?;

        // OutputPath::try_validate enforces size>=1024 at the type boundary (domain pure).
        // Success path also cleans up the per-job temp work_dir; failure path keeps it
        // for post-mortem (audit-idempotency.temp-leak-no-cleanup WARN).
        let len = tokio::fs::metadata(&output).await?.len();
        let (output_path, size_mb) = OutputPath::try_validate(output, len)?;
        drop(tokio::fs::remove_dir_all(&work_dir).await);
        sink.emit(ProgressEvent::done(output_path.0.clone(), size_mb));
        Ok((output_path, size_mb))
    }

    async fn resolve_media(&self, req: &DownloadRequest) -> Result<MediaPlaylist> {
        let (raw_text, base) = self
            .fetch_playlist(&req.input, req.source_url.as_ref(), &req.headers)
            .await?;
        let stripped = png_strip::strip_png_wrapper(&raw_text);
        let normalized = m3u8_normalize::normalize(&String::from_utf8_lossy(stripped));
        match parse_m3u8(normalized.as_bytes(), base.as_ref())? {
            Playlist::Media(m) => Ok(m),
            Playlist::Master { variants } => self.resolve_variant(variants, &req.headers).await,
        }
    }

    async fn fetch_playlist(
        &self,
        input: &M3u8Input,
        source_url: Option<&Url>,
        headers: &HeaderMap,
    ) -> Result<(Vec<u8>, Option<Url>)> {
        match input {
            M3u8Input::Raw(s) => {
                let base = source_url.map(derive_base_url).transpose()?;
                Ok((s.clone().into_bytes(), base))
            }
            M3u8Input::Url(u) => {
                let bytes = self
                    .http
                    .fetch_bytes(HttpRequest::new(u.clone()).with_headers(headers.clone()))
                    .await?;
                Ok((bytes.to_vec(), Some(derive_base_url(u)?)))
            }
            M3u8Input::File(p) => {
                let bytes = tokio::fs::read(p).await?;
                // Derive a `file:///<dir>/` base from the playlist's parent so relative
                // segment paths inside the playlist resolve against disk locations.
                let base = p
                    .canonicalize()
                    .ok()
                    .and_then(|abs| abs.parent().and_then(|d| Url::from_directory_path(d).ok()));
                Ok((bytes, base))
            }
        }
    }

    async fn resolve_variant(
        &self,
        variants: Vec<Variant>,
        headers: &HeaderMap,
    ) -> Result<MediaPlaylist> {
        let best = variants
            .into_iter()
            .max_by_key(|v| v.bandwidth)
            .ok_or_else(|| DownloadError::Parse("master playlist has no variants".into()))?;
        let bytes = self
            .http
            .fetch_bytes(HttpRequest::new(best.uri.clone()).with_headers(headers.clone()))
            .await?;
        let base = derive_base_url(&best.uri)?;
        match parse_m3u8(&bytes, Some(&base))? {
            Playlist::Media(m) => Ok(m),
            Playlist::Master { .. } => Err(DownloadError::Parse("master nested in master".into())),
        }
    }
}

async fn make_temp_workdir() -> Result<PathBuf> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let dir = std::env::temp_dir().join(format!("m3u8dl_{}", &id[..8]));
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}
