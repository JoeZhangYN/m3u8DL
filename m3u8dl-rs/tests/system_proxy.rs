use m3u8dl_server::adapters::system_proxy::parse_proxy_server;

#[test]
fn parses_bare_host_port_as_http() {
    let p = parse_proxy_server("127.0.0.1:7890").expect("ok");
    assert_eq!(p.scheme(), "http");
    assert_eq!(p.host_str(), Some("127.0.0.1"));
    assert_eq!(p.port(), Some(7890));
}

#[test]
fn parses_full_http_url() {
    let p = parse_proxy_server("http://10.0.0.1:8080").expect("ok");
    assert_eq!(p.scheme(), "http");
    assert_eq!(p.host_str(), Some("10.0.0.1"));
    assert_eq!(p.port(), Some(8080));
}

#[test]
fn parses_full_https_url() {
    let p = parse_proxy_server("https://proxy.corp:3128").expect("ok");
    assert_eq!(p.scheme(), "https");
    assert_eq!(p.host_str(), Some("proxy.corp"));
}

#[test]
fn picks_https_from_per_scheme_map() {
    let p = parse_proxy_server("http=h1:1111;https=h2:2222;ftp=h3:3333").expect("ok");
    assert_eq!(p.host_str(), Some("h2"));
    assert_eq!(p.port(), Some(2222));
}

#[test]
fn falls_back_to_http_when_only_http_in_map() {
    let p = parse_proxy_server("http=h1:1111;ftp=h3:3333").expect("ok");
    assert_eq!(p.host_str(), Some("h1"));
    assert_eq!(p.port(), Some(1111));
}

#[test]
fn ignores_socks_only_map() {
    let result = parse_proxy_server("socks=h1:1080");
    assert!(result.is_none(), "should skip socks-only, got {result:?}");
}

#[test]
fn empty_string_yields_none() {
    assert!(parse_proxy_server("").is_none());
    assert!(parse_proxy_server("   ").is_none());
}

#[test]
fn handles_whitespace_around_addr() {
    let p = parse_proxy_server(" http=  h1:1111 ; https= h2:2222 ").expect("ok");
    assert_eq!(p.host_str(), Some("h2"));
}

#[test]
fn malformed_input_yields_none() {
    assert!(parse_proxy_server(":::garbage:::").is_none());
}
