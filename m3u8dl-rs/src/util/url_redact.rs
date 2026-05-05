//! SSOT for "what survives into log lines from URLs". HLS providers commonly use
//! signed query strings (`?token=...&hmac=...&sign=...`) for CDN auth — those tokens
//! are PII / credentials and must never reach log files. This helper drops scheme,
//! userinfo, query, and fragment, keeping only host + path so logs still help
//! diagnose "which segment / which CDN" without leaking the auth material.

use url::Url;

/// Format a URL for logging: `host + path`. Scheme / userinfo / query / fragment dropped.
///
/// ```
/// use m3u8dl_server::util::url_redact::redact_url;
/// use url::Url;
///
/// let u = Url::parse("https://cdn.example.com/v/seg-42.ts?token=secret&hmac=abc").unwrap();
/// assert_eq!(redact_url(&u), "cdn.example.com/v/seg-42.ts");
///
/// // userinfo is dropped too
/// let u = Url::parse("https://user:pass@cdn.example.com/key.bin").unwrap();
/// assert_eq!(redact_url(&u), "cdn.example.com/key.bin");
/// ```
pub fn redact_url(u: &Url) -> String {
    format!("{}{}", u.host_str().unwrap_or(""), u.path())
}
