//! Property tests for `parse_m3u8`. Generated playlists exercise a wider input distribution
//! than the hand-written cases in `parser_media.rs` / `parser_master.rs`, catching:
//! - segment count vs `#EXTINF` count drift
//! - duration sum drift across float coercion
//! - AES-128 key carry-forward across an arbitrary number of segments (RFC 8216 §4.3.2.4)
//! - IV-length validation (must be 32 hex chars after `0x` strip)
//!
//! `Config::with_cases(64)` keeps CI runtime bounded; bump locally with `PROPTEST_CASES`.

use m3u8dl_server::application::parser::parse_m3u8;
use m3u8dl_server::domain::{Encryption, Playlist};
use proptest::prelude::*;
use url::Url;

#[allow(clippy::expect_used)] // tests/* helper
fn base() -> Url {
    Url::parse("https://cdn.example.com/v/").expect("base")
}

fn render_media_playlist(target_duration: u32, durations: &[f32]) -> String {
    let mut s = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
    s.push_str(&format!("#EXT-X-TARGETDURATION:{target_duration}\n"));
    for (i, d) in durations.iter().enumerate() {
        s.push_str(&format!("#EXTINF:{d:.3},\nseg-{i}.ts\n"));
    }
    s.push_str("#EXT-X-ENDLIST\n");
    s
}

fn render_aes_playlist(durations: &[f32], iv_hex: &str) -> String {
    let mut s = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n");
    s.push_str(&format!(
        "#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x{iv_hex}\n"
    ));
    for (i, d) in durations.iter().enumerate() {
        s.push_str(&format!("#EXTINF:{d:.3},\nseg-{i}.ts\n"));
    }
    s.push_str("#EXT-X-ENDLIST\n");
    s
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Segment count == EXTINF count for any valid generated media playlist.
    #[test]
    fn segment_count_matches_extinf_count(
        n in 1usize..=20,
        target in 2u32..=30,
    ) {
        let durations: Vec<f32> = (0..n).map(|i| 1.0 + (i as f32) * 0.5).collect();
        let text = render_media_playlist(target, &durations);
        let pl = parse_m3u8(text.as_bytes(), Some(&base())).expect("parse ok");
        match pl {
            Playlist::Media(m) => prop_assert_eq!(m.segments.len(), n),
            Playlist::Master { .. } => prop_assert!(false, "expected media, got master"),
        }
    }

    /// Sum of EXTINF == total_duration within float tolerance.
    #[test]
    fn total_duration_sums_extinf(
        n in 1usize..=20,
        seed in 0u32..1000,
    ) {
        let durations: Vec<f32> = (0..n).map(|i| 0.5 + ((seed + i as u32) % 50) as f32 * 0.1).collect();
        let expected: f64 = durations.iter().map(|d| f64::from(*d)).sum();
        let text = render_media_playlist(10, &durations);
        let pl = parse_m3u8(text.as_bytes(), Some(&base())).expect("parse ok");
        match pl {
            Playlist::Media(m) => {
                let diff = (m.total_duration - expected).abs();
                prop_assert!(diff < 0.01, "total_duration {} vs sum {} diff {}", m.total_duration, expected, diff);
            }
            Playlist::Master { .. } => prop_assert!(false, "expected media"),
        }
    }

    /// One #EXT-X-KEY at top → ALL segments carry the same Aes128Cbc encryption with that IV.
    /// Audit-source-of-truth gap that motivated this property: RFC 8216 §4.3.2.4 says key carries
    /// forward; m3u8-rs 6.0 attaches the key only to the immediately-following segment, so our
    /// `media_to_domain` does the carry. Regression here would silently break decrypt for
    /// segment 2..N.
    #[test]
    fn aes_key_carries_forward(
        n in 2usize..=15,
        iv_byte in 0u8..=255,
    ) {
        let durations: Vec<f32> = (0..n).map(|_| 4.0).collect();
        let iv_hex = format!("{:032x}", iv_byte as u128);
        let text = render_aes_playlist(&durations, &iv_hex);
        let pl = parse_m3u8(text.as_bytes(), Some(&base())).expect("parse ok");
        match pl {
            Playlist::Media(m) => {
                prop_assert_eq!(m.segments.len(), n);
                for seg in &m.segments {
                    match &seg.encryption {
                        Encryption::Aes128Cbc { iv, .. } => {
                            prop_assert_eq!(iv[15], iv_byte, "seg {} iv last byte mismatch", seg.idx.0);
                        }
                        Encryption::None => prop_assert!(false, "seg {} lost key carry", seg.idx.0),
                    }
                }
            }
            Playlist::Master { .. } => prop_assert!(false, "expected media"),
        }
    }

    /// IV with length != 32 hex chars (after 0x strip) always rejects with Parse error.
    /// Generator deliberately picks lengths that will fail (1..31 and 33..64).
    #[test]
    fn iv_wrong_length_rejects(
        len in prop_oneof![1usize..32, 33usize..=64],
    ) {
        // build a hex string of length `len` from valid hex chars
        let hex_chars = b"0123456789abcdef";
        let bad_iv: String = (0..len).map(|i| hex_chars[i % 16] as char).collect();
        let text = render_aes_playlist(&[4.0, 4.0], &bad_iv);
        let result = parse_m3u8(text.as_bytes(), Some(&base()));
        prop_assert!(result.is_err(), "expected reject for IV len {len}, got {:?}", result.map(|_| "ok"));
    }

    /// Master playlist with N variants → exactly N variants parsed (filter is_i_frame=false handled
    /// by parser; we use plain VARIANT lines that don't trip i-frame).
    #[test]
    fn master_variant_count(
        n in 1usize..=8,
    ) {
        let mut s = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
        for i in 0..n {
            let bw = 500_000 + i * 100_000;
            s.push_str(&format!("#EXT-X-STREAM-INF:BANDWIDTH={bw},RESOLUTION=1280x720\nv{i}/index.m3u8\n"));
        }
        let pl = parse_m3u8(s.as_bytes(), Some(&base())).expect("parse ok");
        match pl {
            Playlist::Master { variants } => prop_assert_eq!(variants.len(), n),
            Playlist::Media(_) => prop_assert!(false, "expected master"),
        }
    }
}
