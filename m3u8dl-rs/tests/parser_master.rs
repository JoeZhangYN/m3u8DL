use m3u8dl_server::application::parser::parse_m3u8;
use m3u8dl_server::domain::Playlist;
use url::Url;

const MASTER: &str = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=720x480,CODECS="avc1.4d401f,mp4a.40.2"
low/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2560000,RESOLUTION=1280x720,CODECS="avc1.4d401f,mp4a.40.2"
mid/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=7680000,RESOLUTION=1920x1080,CODECS="avc1.640028,mp4a.40.2"
high/index.m3u8
"#;

#[test]
fn parses_master_with_three_variants_resolves_relative_uris() {
    let base = Url::parse("https://example.com/video/").expect("base");
    let pl = parse_m3u8(MASTER.as_bytes(), Some(&base)).expect("parse");
    let Playlist::Master { variants } = pl else {
        panic!("expected Master, got Media");
    };
    assert_eq!(variants.len(), 3);
    assert_eq!(variants[0].bandwidth, 1_280_000);
    assert_eq!(variants[2].bandwidth, 7_680_000);
    assert_eq!(variants[2].resolution, Some((1920, 1080)));
    assert_eq!(
        variants[0].uri,
        Url::parse("https://example.com/video/low/index.m3u8").expect("uri")
    );
    assert_eq!(variants[2].codecs.as_deref(), Some("avc1.640028,mp4a.40.2"));
}

#[test]
fn pick_best_variant_returns_max_bandwidth() {
    let base = Url::parse("https://example.com/video/").expect("base");
    let pl = parse_m3u8(MASTER.as_bytes(), Some(&base)).expect("parse");
    let best = pl.pick_best_variant().expect("master has variants");
    assert_eq!(best.bandwidth, 7_680_000);
}

#[test]
fn parser_rejects_relative_uri_without_base() {
    let err = parse_m3u8(MASTER.as_bytes(), None).expect_err("should fail");
    assert!(err.to_string().contains("relative URL"), "got: {err}");
}
