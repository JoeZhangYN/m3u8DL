//! Verify that segment_fetcher::fetch_one short-circuits when the target path already
//! exists with non-zero size (Plan D2). Counts upstream HTTP fetches via a stub
//! HttpClient — pre-populating N-1 segments and running fetch on N segments should
//! result in exactly 1 fetch (the missing one).

#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use m3u8dl_server::application::segment_fetcher::{fetch_one, new_key_cache};
use m3u8dl_server::domain::{Encryption, Result, Segment, SegmentIndex};
use m3u8dl_server::ports::http_client::{HttpClient, HttpRequest};
use reqwest::header::HeaderMap;
use url::Url;

/// Stub HttpClient that counts fetches and returns canned 16-byte payloads.
#[derive(Clone, Default)]
struct CountingClient {
    count: Arc<AtomicUsize>,
}

impl HttpClient for CountingClient {
    fn fetch_bytes(
        &self,
        _req: HttpRequest,
    ) -> impl std::future::Future<Output = Result<Bytes>> + Send + '_ {
        let count = self.count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            // 16 plaintext bytes — must be non-zero so skip-if-present treats it as good
            Ok(Bytes::from_static(b"0123456789abcdef"))
        }
    }
}

#[tokio::test]
async fn skips_fetch_when_segment_file_already_exists() {
    let tmp = tempfile::tempdir().expect("tmp");
    let work_dir = tmp.path();

    // Pre-populate seg-000000.ts with non-zero content (simulating a prior partial run)
    let pre_existing = work_dir.join("seg-000000.ts");
    tokio::fs::write(&pre_existing, b"old-content-non-zero")
        .await
        .expect("pre-populate");

    let client = CountingClient::default();
    let cache = new_key_cache();
    let headers = HeaderMap::new();
    let seg = Segment {
        idx: SegmentIndex(0),
        url: Url::parse("https://x.com/seg-0.ts").expect("url"),
        byte_range: None,
        duration: 5.0,
        encryption: Encryption::None,
    };

    let result = fetch_one(seg, &client, &cache, &headers, work_dir)
        .await
        .expect("fetch");

    // Skip path → 0 upstream fetches
    assert_eq!(client.count.load(Ordering::SeqCst), 0);
    // Returned size matches what we pre-populated (20 bytes)
    assert_eq!(result.bytes_written, 20);
    assert_eq!(result.idx, SegmentIndex(0));
    // File preserved (not overwritten)
    let preserved = tokio::fs::read(&pre_existing).await.expect("read");
    assert_eq!(&preserved[..], b"old-content-non-zero");
}

#[tokio::test]
async fn fetches_when_segment_file_missing() {
    let tmp = tempfile::tempdir().expect("tmp");
    let work_dir = tmp.path();

    let client = CountingClient::default();
    let cache = new_key_cache();
    let headers = HeaderMap::new();
    let seg = Segment {
        idx: SegmentIndex(7),
        url: Url::parse("https://x.com/seg-7.ts").expect("url"),
        byte_range: None,
        duration: 5.0,
        encryption: Encryption::None,
    };

    fetch_one(seg, &client, &cache, &headers, work_dir)
        .await
        .expect("fetch");

    // Did fetch from upstream
    assert_eq!(client.count.load(Ordering::SeqCst), 1);
    // File written
    let path = work_dir.join("seg-000007.ts");
    assert!(path.exists());
}

#[tokio::test]
async fn fetches_when_segment_file_empty() {
    let tmp = tempfile::tempdir().expect("tmp");
    let work_dir = tmp.path();

    // Pre-populate with 0-byte file (simulating a half-write that didn't flush)
    let path = work_dir.join("seg-000003.ts");
    tokio::fs::write(&path, b"").await.expect("touch");

    let client = CountingClient::default();
    let cache = new_key_cache();
    let headers = HeaderMap::new();
    let seg = Segment {
        idx: SegmentIndex(3),
        url: Url::parse("https://x.com/seg-3.ts").expect("url"),
        byte_range: None,
        duration: 5.0,
        encryption: Encryption::None,
    };

    fetch_one(seg, &client, &cache, &headers, work_dir)
        .await
        .expect("fetch");

    // 0-byte file = treat as half-write artifact, re-fetch
    assert_eq!(client.count.load(Ordering::SeqCst), 1);
    let after = tokio::fs::metadata(&path).await.expect("meta");
    assert!(after.len() > 0, "should have re-written");
}
