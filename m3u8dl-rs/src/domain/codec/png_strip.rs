//! PNG-wrapper stripper. Some anti-bot sites prepend a real PNG envelope (signature + chunks
//! through IEND) to the actual payload — both **playlist text** and **media segment bytes**.
//! Browsers/players that stream-parse PNG ignore the trailing data, but our HTTP fetch
//! gets the whole blob — we find the IEND end-of-image marker and skip past it.
//!
//! Call sites (single-source rule — both must use this fn, not reimplement):
//! - `application::download_job` strips the playlist response before parsing
//! - `application::segment_fetcher` strips each segment before AES decrypt / disk write
//!
//! Algorithm (ported from `pipeline.ps1::Strip-PngWrapper`, but applied to segments too —
//! the legacy script delegated segment download to `N_m3u8DL-RE.exe` which handled this
//! internally; the Rust port internalized the download path so the strip is now our job):
//! 1. < 16 bytes → cannot be a wrapped PNG, return as-is
//! 2. No PNG signature (`89 50 4E 47`) → not wrapped, return as-is
//! 3. Search for the IEND chunk type+CRC sequence (`49 45 4E 44 AE 42 60 82`) starting at offset 8
//! 4. If found, return everything after that 8-byte sequence
//! 5. If not found, return as-is (probably a real PNG with no trailer)
//!
//! `&[u8] -> &[u8]` — no allocation, just a slice into the input.

const PNG_SIG: &[u8; 4] = &[0x89, 0x50, 0x4E, 0x47];

/// IEND chunk-type (`I E N D`) + the fixed CRC32 of an empty IEND chunk (`AE 42 60 82`).
/// Together these 8 bytes mark the end of every valid PNG file.
const IEND_END: &[u8; 8] = &[0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82];

/// Strip a PNG wrapper if present. Returns the original slice if input does not look wrapped.
pub fn strip_png_wrapper(bytes: &[u8]) -> &[u8] {
    if bytes.len() < 16 {
        return bytes;
    }
    if &bytes[0..4] != PNG_SIG {
        return bytes;
    }
    // Search for IEND from offset 8 (after PNG signature + first chunk length field).
    if let Some(pos) = find_subslice(&bytes[8..], IEND_END) {
        let abs = 8 + pos + IEND_END.len();
        return &bytes[abs..];
    }
    bytes
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
