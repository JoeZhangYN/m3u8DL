//! Normalize a (possibly mangled) m3u8 text blob into a parser-acceptable form.
//!
//! Two real-world quirks this fixes (ported from `pipeline.ps1::ConvertTo-NormalizedM3u8`):
//! 1. **DevTools "copy as text" smashes the playlist into one line** —
//!    if `#EXTM3U` is present and the blob has < 5 logical lines, insert `\n` before every
//!    `#EXT…` directive and every `https?://` URL.
//! 2. **Live-mode m3u8 (no `#EXT-X-ENDLIST`) confuses N_m3u8DL-RE / m3u8-rs into "live" mode** —
//!    if `#EXT-X-ENDLIST` is missing, append it.

use std::sync::LazyLock;

use regex::Regex;

#[allow(clippy::expect_used)] // static literal regex; if it fails to compile, the bug is in source
static EXT_LOOKBEHIND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+(#EXT)").expect("EXT_LOOKBEHIND regex literal valid"));

#[allow(clippy::expect_used)] // see above
static URL_LOOKBEHIND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+(https?://)").expect("URL_LOOKBEHIND regex literal valid"));

/// Apply the two normalizations in order. Idempotent: re-applying yields the same string.
pub fn normalize(text: &str) -> String {
    let mut out = text.to_string();
    if out.contains("#EXTM3U") {
        let line_count = out.split(['\n', '\r']).count();
        if line_count < 5 {
            out = EXT_LOOKBEHIND.replace_all(&out, "\n$1").into_owned();
            out = URL_LOOKBEHIND.replace_all(&out, "\n$1").into_owned();
            out = out.trim().to_string();
        }
    }
    if !out.contains("#EXT-X-ENDLIST") {
        // No trailing newline — spec allows either; omitting it keeps `normalize` idempotent
        // (a second pass would otherwise trim the `\n` then fail to re-detect ENDLIST… no, the
        // contains check still fires; but trim_end on the whole result keeps the canonical form
        // stable across multiple applications).
        out = format!("{}\n#EXT-X-ENDLIST", out.trim_end());
    }
    out
}
