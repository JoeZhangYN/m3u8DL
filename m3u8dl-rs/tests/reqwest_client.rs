use std::time::Duration;

use m3u8dl_server::adapters::reqwest_client::ReqwestClient;
use m3u8dl_server::ports::http_client::{HttpClient, HttpRequest};
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[allow(clippy::expect_used)] // helper for #[tokio::test] fns; clippy's allow-expect-in-tests
                              // doesn't see free-standing helpers in tests/*.rs (only `#[cfg(test)]`)
fn make_client(retries: u32) -> ReqwestClient {
    ReqwestClient::new().expect("client").with_max_retries(retries)
}

#[tokio::test]
async fn fetches_bytes_on_first_try() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/seg-0.ts"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"abcdef".as_ref()))
        .mount(&mock)
        .await;

    let client = make_client(0);
    let url = Url::parse(&format!("{}/seg-0.ts", mock.uri())).expect("url");
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        client.fetch_bytes(HttpRequest::new(url)),
    )
    .await
    .expect("no hang")
    .expect("ok");
    assert_eq!(&bytes[..], b"abcdef");
}

#[tokio::test]
async fn retries_then_succeeds() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/flaky"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/flaky"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"win".as_ref()))
        .mount(&mock)
        .await;

    let client = make_client(3);
    let url = Url::parse(&format!("{}/flaky", mock.uri())).expect("url");
    let bytes = tokio::time::timeout(
        Duration::from_secs(15),
        client.fetch_bytes(HttpRequest::new(url)),
    )
    .await
    .expect("no hang")
    .expect("retry should succeed");
    assert_eq!(&bytes[..], b"win");
}

#[tokio::test]
async fn returns_error_after_max_retries() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/dead"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock)
        .await;

    let client = make_client(2);
    let url = Url::parse(&format!("{}/dead", mock.uri())).expect("url");
    let err = tokio::time::timeout(
        Duration::from_secs(10),
        client.fetch_bytes(HttpRequest::new(url)),
    )
    .await
    .expect("no hang")
    .expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("network") || msg.contains("503"), "got: {msg}");
}

#[tokio::test]
async fn passes_range_header() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ranged"))
        .and(header("Range", "bytes=100-199"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(b"sliced".as_ref()))
        .mount(&mock)
        .await;

    let client = make_client(0);
    let url = Url::parse(&format!("{}/ranged", mock.uri())).expect("url");
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        client.fetch_bytes(HttpRequest::new(url).with_range(100, 199)),
    )
    .await
    .expect("no hang")
    .expect("ok");
    assert_eq!(&bytes[..], b"sliced");
}

#[tokio::test]
async fn fetch_text_decodes_utf8() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/playlist.m3u8"))
        .respond_with(ResponseTemplate::new(200).set_body_string("#EXTM3U\nseg.ts\n"))
        .mount(&mock)
        .await;

    let client = make_client(0);
    let url = Url::parse(&format!("{}/playlist.m3u8", mock.uri())).expect("url");
    let text = tokio::time::timeout(
        Duration::from_secs(5),
        client.fetch_text(HttpRequest::new(url)),
    )
    .await
    .expect("no hang")
    .expect("ok");
    assert!(text.starts_with("#EXTM3U"));
}
