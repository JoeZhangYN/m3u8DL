//! HTTP client port. The application layer fetches m3u8 text + segment bytes through this trait,
//! never touching `reqwest` directly. Adapter impl lives in `adapters::reqwest_client`; tests
//! plug in a `wiremock`-backed `ReqwestClient` against a local mock server.
//!
//! `async fn in trait` (Rust 1.75+) — we accept the dyn-incompatibility tradeoff because we
//! always parameterize the orchestrator with a concrete `<C: HttpClient>` (no `Box<dyn>`).

use bytes::Bytes;
use reqwest::header::HeaderMap;
use url::Url;

use crate::domain::Result;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: Url,
    pub headers: HeaderMap,
    pub range: Option<RangeSpec>,
}

impl HttpRequest {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            headers: HeaderMap::new(),
            range: None,
        }
    }
    pub fn with_headers(mut self, h: HeaderMap) -> Self {
        self.headers = h;
        self
    }
    pub fn with_range(mut self, start: u64, end: u64) -> Self {
        self.range = Some(RangeSpec { start, end });
        self
    }
}

/// Inclusive byte range — `bytes=start-end` HTTP header semantics.
#[derive(Debug, Clone, Copy)]
pub struct RangeSpec {
    pub start: u64,
    pub end: u64,
}

// Trait kept dyn-incompatible (orchestrator generic over `<C: HttpClient>`); the explicit
// `+ Send + '_` bounds on the returned Future are needed so DownloadJob's run() future
// is `Send`, which is in turn required for `Arc<dyn DownloadOrchestrator>` (see ports/orchestrator.rs).
pub trait HttpClient: Send + Sync {
    fn fetch_bytes(
        &self,
        req: HttpRequest,
    ) -> impl std::future::Future<Output = Result<Bytes>> + Send + '_;

    /// Default impl: fetch bytes, decode UTF-8.
    fn fetch_text(
        &self,
        req: HttpRequest,
    ) -> impl std::future::Future<Output = Result<String>> + Send + '_ {
        async move {
            let bytes = self.fetch_bytes(req).await?;
            String::from_utf8(bytes.to_vec())
                .map_err(|e| crate::domain::DownloadError::Parse(format!("utf8 decode: {e}")))
        }
    }
}
