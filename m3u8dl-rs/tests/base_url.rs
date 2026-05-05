use m3u8dl_server::application::base_url::derive_base_url;
use proptest::prelude::*;
use url::Url;

#[allow(clippy::expect_used)] // free-standing helper in tests/*.rs
fn parse(s: &str) -> Url {
    Url::parse(s).expect("test url")
}

#[test]
fn strips_m3u8_filename() {
    let m = parse("https://cdn.example.com/some/path/index.m3u8");
    let base = derive_base_url(&m).expect("base");
    assert_eq!(base.as_str(), "https://cdn.example.com/some/path/");
}

#[test]
fn strips_query_and_fragment() {
    let m = parse("https://cdn.example.com/v/index.m3u8?token=xxx&t=42#anchor");
    let base = derive_base_url(&m).expect("base");
    assert_eq!(base.as_str(), "https://cdn.example.com/v/");
}

#[test]
fn root_level_m3u8_yields_root_base() {
    let m = parse("https://cdn.example.com/index.m3u8");
    let base = derive_base_url(&m).expect("base");
    assert_eq!(base.as_str(), "https://cdn.example.com/");
}

#[test]
fn already_a_directory_url_unchanged_modulo_trailing_slash() {
    let m = parse("https://cdn.example.com/v/");
    let base = derive_base_url(&m).expect("base");
    // popping a trailing-empty segment of "/v/" gives "/v" — pushing "" restores "/v/"
    // but URL canonicalisation may render it differently; key invariant is that joining
    // a relative path resolves under /v/, not / .
    let joined = base.join("seg-0.ts").expect("join");
    assert_eq!(joined.as_str(), "https://cdn.example.com/v/seg-0.ts");
}

#[test]
fn base_resolves_relative_segment_correctly() {
    let m = parse("https://cdn.example.com/some/path/index.m3u8?h=abc");
    let base = derive_base_url(&m).expect("base");
    let seg = base.join("seg-0.ts?h=def").expect("join");
    assert_eq!(
        seg.as_str(),
        "https://cdn.example.com/some/path/seg-0.ts?h=def"
    );
}

#[test]
fn base_resolves_absolute_path_segment() {
    // segment with leading "/" should resolve at host root, not under playlist path
    let m = parse("https://cdn.example.com/v/index.m3u8");
    let base = derive_base_url(&m).expect("base");
    let seg = base.join("/abs/seg.ts").expect("join");
    assert_eq!(seg.as_str(), "https://cdn.example.com/abs/seg.ts");
}

#[test]
fn rejects_cannot_be_a_base_url() {
    // mailto: scheme cannot be a base URL
    let m = Url::parse("mailto:user@example.com").expect("url");
    let err = derive_base_url(&m).expect_err("should reject");
    assert!(err.to_string().contains("cannot be a base"), "got: {err}");
}

proptest! {
    #[test]
    fn property_join_relative_under_base(
        host in "[a-z]{1,10}\\.example\\.com",
        path in "[a-z]{1,8}/[a-z]{1,8}",
        filename in "[a-z]{1,12}\\.ts",
    ) {
        let m3u8_url = format!("https://{host}/{path}/index.m3u8");
        let m = Url::parse(&m3u8_url).unwrap();
        let base = derive_base_url(&m).unwrap();
        let joined = base.join(&filename).unwrap();
        let expected = format!("https://{host}/{path}/{filename}");
        prop_assert_eq!(joined.as_str(), &expected);
    }

    #[test]
    fn property_query_always_dropped(
        path in "[a-z]{1,10}/[a-z]{1,8}\\.m3u8",
        token in "[a-zA-Z0-9]{1,20}",
    ) {
        let m3u8_url = format!("https://cdn.example.com/{path}?token={token}");
        let m = Url::parse(&m3u8_url).unwrap();
        let base = derive_base_url(&m).unwrap();
        prop_assert!(base.query().is_none(), "base still has query: {base}");
    }
}
