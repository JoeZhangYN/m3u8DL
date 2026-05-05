//! `HttpClient` impl backed by `reqwest` with exponential-backoff retry.
//!
//! Retry policy: `max_retries = 3` retries (4 total attempts), backoff 250→500→1000→2000ms
//! capped at 2s, with ±50% jitter to avoid thundering-herd on shared CDN failures.
//! Only retryable errors trigger retry — see `is_retryable`. Permanent errors (4xx, DNS,
//! TLS handshake) return immediately on attempt 1.

use std::time::Duration;

use bytes::Bytes;

use crate::adapters::system_proxy;
use crate::domain::{DownloadError, Result};
use crate::ports::http_client::{HttpClient, HttpRequest};
use crate::util::url_redact::redact_url;

/// Classify a `DownloadError` for retry: only transient transport + 5xx + 429 are retryable.
/// Permanent client errors (4xx other than 429), DNS resolution failure (NXDOMAIN), and TLS
/// handshake errors fail immediately so callers don't waste backoff budget on guaranteed
/// failures (audit-resilience.retry-non-retryable-errors FAIL). `fetch_once` only ever
/// returns `Network(reqwest::Error)` — other variants short-circuit to `false` defensively.
fn is_retryable_err(e: &DownloadError) -> bool {
    let DownloadError::Network(re) = e else {
        return false;
    };
    if re.is_timeout() || re.is_connect() || re.is_request() {
        return true;
    }
    if let Some(s) = re.status() {
        return s.is_server_error() || s == reqwest::StatusCode::TOO_MANY_REQUESTS;
    }
    false
}

#[derive(Clone)]
pub struct ReqwestClient {
    client: reqwest::Client,
    max_retries: u32,
}

impl ReqwestClient {
    /// Build a client with proxy precedence:
    /// 1. `M3U8DL_PROXY=none` (or empty) → no proxy
    /// 2. `M3U8DL_PROXY=<url>`           → use that proxy
    /// 3. Windows system proxy (WinINET) → auto-detect
    /// 4. Otherwise direct
    pub fn new() -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Some(Duration::from_secs(60)));

        match std::env::var("M3U8DL_PROXY")
            .ok()
            .map(|s| s.trim().to_string())
        {
            Some(v) if v.eq_ignore_ascii_case("none") || v.is_empty() => {
                tracing::info!("proxy: disabled via M3U8DL_PROXY={}", v);
                builder = builder.no_proxy();
            }
            Some(v) => match reqwest::Proxy::all(&v) {
                Ok(proxy) => {
                    tracing::info!("proxy: M3U8DL_PROXY={}", v);
                    builder = builder.proxy(proxy);
                }
                Err(e) => tracing::warn!("proxy: M3U8DL_PROXY={} parse failed ({e}), ignoring", v),
            },
            None => match system_proxy::detect_system_proxy() {
                Some(url) => match reqwest::Proxy::all(url.as_str()) {
                    Ok(proxy) => {
                        tracing::info!("proxy: system proxy {url}");
                        builder = builder.no_proxy().proxy(proxy);
                    }
                    Err(e) => {
                        tracing::warn!("proxy: system proxy {url} parse failed ({e}), going direct")
                    }
                },
                None => tracing::info!("proxy: none (no system proxy, no M3U8DL_PROXY)"),
            },
        }

        let client = builder.build()?;
        Ok(Self {
            client,
            max_retries: 3,
        })
    }

    /// Override the retry count (default 3). Useful for tests.
    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    async fn fetch_once(&self, req: &HttpRequest) -> Result<Bytes> {
        let mut builder = self
            .client
            .get(req.url.clone())
            .headers(req.headers.clone());
        if let Some(r) = req.range {
            builder = builder.header("Range", format!("bytes={}-{}", r.start, r.end));
        }
        let resp = builder.send().await?.error_for_status()?;
        Ok(resp.bytes().await?)
    }
}

impl HttpClient for ReqwestClient {
    async fn fetch_bytes(&self, req: HttpRequest) -> Result<Bytes> {
        // SOT for retry backoff timings (max_retries=3 retries → 4 total attempts; backoff
        // 250 → 500 → 1000 → 2000ms cap with ±50% jitter; non-retryable errors fail fast).
        // Synced doc copies: README.md "✨ 特性" + m3u8dl-rs/docs/SCOPE.md "网络". Update those if changed.
        let mut backoff_ms = 250u64;
        let mut attempt = 0u32;
        loop {
            match self.fetch_once(&req).await {
                Ok(bytes) => return Ok(bytes),
                Err(e) if attempt < self.max_retries && is_retryable_err(&e) => {
                    let jitter = 0.5 + rand::random::<f64>(); // 0.5..=1.5
                    let actual_ms = (backoff_ms as f64 * jitter) as u64;
                    tracing::warn!(
                        url = %redact_url(&req.url),
                        attempt,
                        backoff_ms = actual_ms,
                        error = %e,
                        "fetch failed, retrying"
                    );
                    tokio::time::sleep(Duration::from_millis(actual_ms)).await;
                    backoff_ms = backoff_ms.saturating_mul(2).min(2000);
                    attempt += 1;
                }
                Err(e) => {
                    tracing::error!(
                        url = %redact_url(&req.url),
                        total_attempts = attempt + 1,
                        retryable = is_retryable_err(&e),
                        error = %e,
                        "fetch failed (terminal)"
                    );
                    return Err(e);
                }
            }
        }
    }
}
