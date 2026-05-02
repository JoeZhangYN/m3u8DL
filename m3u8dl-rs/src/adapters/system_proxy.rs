//! Read the Windows system proxy from `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`.
//!
//! Three real-world `ProxyServer` formats exist (per RUST_PORT_REFERENCE.md §4):
//! 1. `127.0.0.1:7890`                  — bare host:port (assumed HTTP)
//! 2. `http=h:p;https=h:p;ftp=h:p`     — per-scheme map (we prefer https, fall back to http)
//! 3. `http://127.0.0.1:7890`           — full URL
//!
//! `socks=...` is silently skipped — reqwest can support socks but the legacy PS pipeline
//! drops it, and the source sites observed need HTTP CONNECT only.

use url::Url;

#[cfg(windows)]
pub fn detect_system_proxy() -> Option<Url> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let settings = hkcu
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = settings.get_value("ProxyEnable").ok()?;
    if enabled == 0 {
        return None;
    }
    let server: String = settings.get_value("ProxyServer").ok()?;
    parse_proxy_server(&server)
}

#[cfg(not(windows))]
pub fn detect_system_proxy() -> Option<Url> {
    None
}

/// Pure parser — testable in isolation, no registry / OS deps.
pub fn parse_proxy_server(s: &str) -> Option<Url> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        return Url::parse(s).ok();
    }
    if s.contains('=') {
        let (mut https_addr, mut http_addr) = (None, None);
        for part in s.split(';') {
            if let Some((scheme, addr)) = part.split_once('=') {
                match scheme.trim().to_lowercase().as_str() {
                    "https" => https_addr = Some(addr.trim()),
                    "http" => http_addr = Some(addr.trim()),
                    _ => {} // skip socks / ftp / others
                }
            }
        }
        return https_addr.or(http_addr).and_then(parse_bare_or_url);
    }
    parse_bare_or_url(s)
}

fn parse_bare_or_url(s: &str) -> Option<Url> {
    if s.starts_with("http://") || s.starts_with("https://") {
        Url::parse(s).ok()
    } else {
        Url::parse(&format!("http://{s}")).ok()
    }
}
