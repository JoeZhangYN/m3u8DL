use m3u8dl_server::domain::M3u8Input;

#[test]
fn detects_raw_when_starts_with_extm3u() {
    let s = "#EXTM3U\n#EXT-X-VERSION:3\nseg.ts\n";
    let i = M3u8Input::detect(s).expect("ok");
    match i {
        M3u8Input::Raw(t) => assert!(t.starts_with("#EXTM3U")),
        other => panic!("expected Raw, got {other:?}"),
    }
}

#[test]
fn detects_url_for_https() {
    let s = "https://cdn.example.com/v/index.m3u8?token=xxx";
    let i = M3u8Input::detect(s).expect("ok");
    match i {
        M3u8Input::Url(u) => assert_eq!(u.host_str(), Some("cdn.example.com")),
        other => panic!("expected Url, got {other:?}"),
    }
}

#[test]
fn detects_url_for_http() {
    let s = "http://localhost:8080/stream.m3u8";
    let i = M3u8Input::detect(s).expect("ok");
    assert!(matches!(i, M3u8Input::Url(_)));
}

#[test]
fn detects_file_for_existing_path() {
    let tmp = tempfile::NamedTempFile::new().expect("temp");
    let i = M3u8Input::detect(&tmp.path().to_string_lossy()).expect("ok");
    assert!(matches!(i, M3u8Input::File(_)));
}

#[test]
fn rejects_unknown_input_with_preview() {
    let s = "this is not a playlist nor URL nor path";
    let err = M3u8Input::detect(s).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("preview"), "got: {msg}");
    assert!(msg.contains("this is not a playlist"), "got: {msg}");
}

#[test]
fn rejects_non_http_scheme() {
    let s = "ftp://example.com/file.m3u8";
    let err = M3u8Input::detect(s).expect_err("should fail");
    assert!(err.to_string().contains("preview"), "got: {err}");
}

#[test]
fn rejects_mailto_url() {
    let s = "mailto:nope@example.com";
    let err = M3u8Input::detect(s).expect_err("should fail");
    assert!(err.to_string().contains("preview"), "got: {err}");
}

#[test]
fn unknown_input_preview_truncated_to_80_chars() {
    let s: String = "x".repeat(500);
    let err = M3u8Input::detect(&s).expect_err("should fail");
    let msg = err.to_string();
    // preview should not contain all 500 x's
    let xs_in_msg = msg.matches('x').count();
    assert!(
        xs_in_msg <= 100,
        "preview too long: {xs_in_msg} x's in message"
    );
}
