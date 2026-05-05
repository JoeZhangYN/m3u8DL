use m3u8dl_server::application::parser::parse_m3u8;
use m3u8dl_server::domain::{Encryption, Playlist};
use url::Url;

const PLAIN: &str = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:6
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:5.760,
seg-0.ts
#EXTINF:5.760,
seg-1.ts
#EXTINF:5.760,
seg-2.ts
#EXT-X-ENDLIST
"#;

const AES128: &str = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-KEY:METHOD=AES-128,URI="key.bin",IV=0x0123456789abcdef0123456789abcdef
#EXTINF:9.97,
seg-0.ts
#EXTINF:9.97,
seg-1.ts
#EXT-X-ENDLIST
"#;

const SAMPLE_AES: &str = r#"#EXTM3U
#EXT-X-VERSION:5
#EXT-X-TARGETDURATION:6
#EXT-X-KEY:METHOD=SAMPLE-AES,URI="skd://example.com/key",KEYFORMAT="com.apple.streamingkeydelivery"
#EXTINF:5.76,
seg-0.ts
#EXT-X-ENDLIST
"#;

const WITH_INIT: &str = r#"#EXTM3U
#EXT-X-VERSION:7
#EXT-X-TARGETDURATION:6
#EXT-X-MAP:URI="init.mp4"
#EXTINF:5.76,
seg-0.m4s
#EXTINF:5.76,
seg-1.m4s
#EXT-X-ENDLIST
"#;

#[test]
fn parses_plain_media_with_three_segments() {
    let base = Url::parse("https://example.com/v/").expect("base");
    let pl = parse_m3u8(PLAIN.as_bytes(), Some(&base)).expect("parse");
    let Playlist::Media(media) = pl else {
        panic!("expected Media");
    };
    assert_eq!(media.segments.len(), 3);
    assert_eq!(media.target_duration, 6);
    assert!(
        (media.total_duration - 17.28).abs() < 0.01,
        "got {}",
        media.total_duration
    );
    assert_eq!(
        media.segments[0].url,
        Url::parse("https://example.com/v/seg-0.ts").expect("uri")
    );
    assert!(matches!(media.segments[0].encryption, Encryption::None));
    assert_eq!(media.segments[2].idx.0, 2);
}

#[test]
fn parses_aes128_key_carries_to_all_segments() {
    let base = Url::parse("https://example.com/v/").expect("base");
    let pl = parse_m3u8(AES128.as_bytes(), Some(&base)).expect("parse");
    let Playlist::Media(media) = pl else {
        panic!("expected Media");
    };
    assert_eq!(media.segments.len(), 2);
    let expected_iv = [
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd,
        0xef,
    ];
    for seg in &media.segments {
        let Encryption::Aes128Cbc { key_uri, iv } = &seg.encryption else {
            panic!("seg {:?}: expected Aes128Cbc", seg.idx);
        };
        assert_eq!(
            key_uri,
            &Url::parse("https://example.com/v/key.bin").expect("key uri")
        );
        assert_eq!(iv, &expected_iv);
    }
}

#[test]
fn rejects_sample_aes_with_unsupported() {
    let base = Url::parse("https://example.com/").expect("base");
    let err = parse_m3u8(SAMPLE_AES.as_bytes(), Some(&base)).expect_err("should fail");
    let msg = err.to_string();
    assert!(msg.contains("unsupported"), "got: {msg}");
    assert!(msg.contains("SAMPLE-AES"), "got: {msg}");
    assert!(msg.contains("docs/SCOPE.md"), "got: {msg}");
}

#[test]
fn parses_init_segment_from_ext_x_map() {
    let base = Url::parse("https://example.com/v/").expect("base");
    let pl = parse_m3u8(WITH_INIT.as_bytes(), Some(&base)).expect("parse");
    let Playlist::Media(media) = pl else {
        panic!("expected Media");
    };
    let init = media.init.as_ref().expect("init segment");
    assert_eq!(
        init.url,
        Url::parse("https://example.com/v/init.mp4").expect("init uri")
    );
}

#[test]
fn rejects_aes128_without_iv() {
    let m = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:5.76,\nseg-0.ts\n#EXT-X-ENDLIST\n";
    let base = Url::parse("https://example.com/v/").expect("base");
    let err = parse_m3u8(m.as_bytes(), Some(&base)).expect_err("should fail");
    assert!(err.to_string().contains("IV"), "got: {err}");
}
