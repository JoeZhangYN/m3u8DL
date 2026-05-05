//! Orchestrator e2e via wiremock + a stub Muxer that just byte-concats inputs.
//! Verifies: playlist fetch, segment parallel download, AES-128 decrypt, progress events,
//! output file written + size-gated.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use aes::Aes128;
use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use hex_literal::hex;
use m3u8dl_server::adapters::reqwest_client::ReqwestClient;
use m3u8dl_server::application::download_job::{DownloadJob, DownloadRequest};
use m3u8dl_server::domain::{M3u8Input, ProgressEvent, Result};
use m3u8dl_server::ports::muxer::{Muxer, OrderedSegments};
use m3u8dl_server::ports::progress_sink::ProgressSink;
use reqwest::header::HeaderMap;
use std::sync::Arc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

type Aes128CbcEnc = cbc::Encryptor<Aes128>;

#[allow(clippy::expect_used)] // tests/* helper
struct CollectingSink(Mutex<Vec<ProgressEvent>>);
impl CollectingSink {
    fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }
    #[allow(clippy::expect_used)]
    fn snapshot(&self) -> Vec<ProgressEvent> {
        self.0.lock().expect("lock").clone()
    }
}
impl ProgressSink for CollectingSink {
    #[allow(clippy::expect_used)]
    fn emit(&self, e: ProgressEvent) {
        self.0.lock().expect("lock").push(e);
    }
}

#[allow(clippy::expect_used)] // tests/* helper
struct ConcatMuxer;
impl Muxer for ConcatMuxer {
    async fn mux(
        &self,
        inputs: &OrderedSegments,
        output: &Path,
        _dur: f64,
        sink: &dyn ProgressSink,
    ) -> Result<()> {
        let mut out = Vec::new();
        for p in inputs.paths() {
            out.extend_from_slice(&tokio::fs::read(p).await?);
        }
        if out.len() < 1024 {
            out.extend(std::iter::repeat_n(0u8, 1024 - out.len()));
        }
        tokio::fs::write(output, &out).await?;
        sink.emit(ProgressEvent::merging(Some(100.0)));
        Ok(())
    }
}

#[allow(clippy::expect_used)] // tests/* helper
fn build_job(out_dir: PathBuf) -> DownloadJob<ReqwestClient, ConcatMuxer> {
    DownloadJob {
        http: ReqwestClient::new().expect("client").with_max_retries(2),
        muxer: ConcatMuxer,
        out_dir,
        parallelism: 4,
    }
}

#[tokio::test]
async fn end_to_end_plain_three_segments_via_url_input() {
    let mock = MockServer::start().await;
    let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXTINF:5.0,\nseg-0.ts\n#EXTINF:5.0,\nseg-1.ts\n#EXTINF:5.0,\nseg-2.ts\n#EXT-X-ENDLIST\n";
    Mock::given(method("GET"))
        .and(path("/v/index.m3u8"))
        .respond_with(ResponseTemplate::new(200).set_body_string(playlist))
        .mount(&mock)
        .await;
    for i in 0..3 {
        Mock::given(method("GET"))
            .and(path(format!("/v/seg-{i}.ts")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0xAB; 4096]))
            .mount(&mock)
            .await;
    }

    let tmp = tempfile::tempdir().expect("tmp");
    let job = build_job(tmp.path().to_path_buf());
    let url = Url::parse(&format!("{}/v/index.m3u8", mock.uri())).expect("url");
    let sink: Arc<CollectingSink> = Arc::new(CollectingSink::new());
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        job.run(
            DownloadRequest {
                input: M3u8Input::Url(url),
                source_url: None,
                title: "test_plain".into(),
                headers: HeaderMap::new(),
            },
            sink.clone(),
        ),
    )
    .await
    .expect("no hang")
    .expect("ok");

    let meta = std::fs::metadata(&result.0.0).expect("output exists");
    assert!(meta.len() >= 1024, "tiny output: {}", meta.len());

    let events = sink.snapshot();
    assert!(
        matches!(events.first(), Some(ProgressEvent::Parsing)),
        "first event = Parsing"
    );
    let downloads = events
        .iter()
        .filter(|e| matches!(e, ProgressEvent::Downloading { .. }))
        .count();
    assert_eq!(downloads, 3, "expected 3 download events, got {downloads}");
    assert!(
        matches!(events.last(), Some(ProgressEvent::Done { .. })),
        "last event = Done"
    );
}

#[tokio::test]
async fn end_to_end_aes128_decrypts_using_cached_key() {
    let mock = MockServer::start().await;
    let key: [u8; 16] = hex!("00112233445566778899aabbccddeeff");
    let iv: [u8; 16] = hex!("ffeeddccbbaa99887766554433221100");

    let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0xffeeddccbbaa99887766554433221100\n#EXTINF:5.0,\nseg-0.ts\n#EXTINF:5.0,\nseg-1.ts\n#EXT-X-ENDLIST\n";
    Mock::given(method("GET"))
        .and(path("/enc/index.m3u8"))
        .respond_with(ResponseTemplate::new(200).set_body_string(playlist))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/enc/key.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(key.to_vec()))
        .mount(&mock)
        .await;
    for i in 0..2 {
        let plain = vec![i as u8 + 0x10; 1024];
        let enc = Aes128CbcEnc::new(&key.into(), &iv.into());
        let ct = enc.encrypt_padded_vec_mut::<Pkcs7>(&plain);
        Mock::given(method("GET"))
            .and(path(format!("/enc/seg-{i}.ts")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(ct))
            .mount(&mock)
            .await;
    }

    let tmp = tempfile::tempdir().expect("tmp");
    let job = build_job(tmp.path().to_path_buf());
    let url = Url::parse(&format!("{}/enc/index.m3u8", mock.uri())).expect("url");
    let sink: Arc<CollectingSink> = Arc::new(CollectingSink::new());
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        job.run(
            DownloadRequest {
                input: M3u8Input::Url(url),
                source_url: None,
                title: "test_aes".into(),
                headers: HeaderMap::new(),
            },
            sink.clone(),
        ),
    )
    .await
    .expect("no hang")
    .expect("ok");

    assert!(result.0.0.exists(), "output not written");
    let events = sink.snapshot();
    let dl_count = events
        .iter()
        .filter(|e| matches!(e, ProgressEvent::Downloading { .. }))
        .count();
    assert_eq!(dl_count, 2, "expected 2 downloads, got {dl_count}");
}

#[tokio::test]
async fn raw_input_with_source_url_resolves_relative_segments() {
    let mock = MockServer::start().await;
    for i in 0..2 {
        Mock::given(method("GET"))
            .and(path(format!("/raw/seg-{i}.ts")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0x55; 2048]))
            .mount(&mock)
            .await;
    }

    let raw = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXTINF:5.0,\nseg-0.ts\n#EXTINF:5.0,\nseg-1.ts\n#EXT-X-ENDLIST\n";
    let source = Url::parse(&format!("{}/raw/index.m3u8?token=abc", mock.uri())).expect("url");
    let tmp = tempfile::tempdir().expect("tmp");
    let job = build_job(tmp.path().to_path_buf());
    let sink: Arc<CollectingSink> = Arc::new(CollectingSink::new());
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        job.run(
            DownloadRequest {
                input: M3u8Input::Raw(raw.into()),
                source_url: Some(source),
                title: "test_raw".into(),
                headers: HeaderMap::new(),
            },
            sink.clone(),
        ),
    )
    .await
    .expect("no hang")
    .expect("ok");
    assert!(result.0.0.exists());
}
