//! Parse m3u8 bytes into our `domain::Playlist` ADT.
//!
//! Wraps `m3u8-rs` and:
//! - resolves relative URLs against an optional base
//! - converts `KeyMethod` → `Encryption` (Aes128 supported, SampleAES/Other → `Unsupported`)
//! - merges `#EXT-X-MAP` (we keep the first one — multiple maps with discontinuities are rare and
//!   out-of-scope for Tier 1; raise `Unsupported` if we hit them in the wild)
//! - sums `#EXTINF` for `total_duration`

use url::Url;

use crate::domain::{
    ByteRange, DownloadError, Encryption, InitSegment, MediaPlaylist, Playlist, Result, Segment,
    SegmentIndex, Variant,
};

pub fn parse_m3u8(bytes: &[u8], base_url: Option<&Url>) -> Result<Playlist> {
    let parsed = m3u8_rs::parse_playlist_res(bytes)
        .map_err(|e| DownloadError::Parse(format!("m3u8 parse: {e}")))?;

    match parsed {
        m3u8_rs::Playlist::MasterPlaylist(m) => master_to_domain(m, base_url),
        m3u8_rs::Playlist::MediaPlaylist(m) => media_to_domain(m, base_url),
    }
}

fn master_to_domain(m: m3u8_rs::MasterPlaylist, base: Option<&Url>) -> Result<Playlist> {
    let variants: Vec<Variant> = m
        .variants
        .into_iter()
        .filter(|v| !v.is_i_frame) // I-frame-only streams are useless for VOD download
        .map(|v| {
            Ok(Variant {
                uri: resolve_url(&v.uri, base)?,
                bandwidth: v.bandwidth,
                resolution: v.resolution.map(|r| (r.width as u32, r.height as u32)),
                codecs: v.codecs,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Playlist::Master { variants })
}

fn media_to_domain(m: m3u8_rs::MediaPlaylist, base: Option<&Url>) -> Result<Playlist> {
    let mut segments = Vec::with_capacity(m.segments.len());
    let mut init: Option<InitSegment> = None;
    let mut total_duration = 0.0_f64;
    // m3u8-rs 6.0 attaches `key`/`map` only to the segment immediately following the tag —
    // it does NOT carry forward as required by RFC 8216 §4.3.2.4. We do the carry here.
    let mut current_encryption = Encryption::None;

    for (idx, seg) in m.segments.into_iter().enumerate() {
        let url = resolve_url(&seg.uri, base)?;
        if let Some(k) = &seg.key {
            current_encryption = parse_key(k, base)?;
        }
        let encryption = current_encryption.clone();
        let byte_range = seg.byte_range.map(|r| ByteRange { length: r.length, offset: r.offset });
        if let Some(map) = seg.map
            && init.is_none()
        {
            init = Some(InitSegment {
                url: resolve_url(&map.uri, base)?,
                byte_range: map.byte_range.map(|r| ByteRange { length: r.length, offset: r.offset }),
            });
        }
        total_duration += f64::from(seg.duration);
        segments.push(Segment {
            idx: SegmentIndex(idx as u32),
            url,
            byte_range,
            duration: seg.duration,
            encryption,
        });
    }

    Ok(Playlist::Media(MediaPlaylist {
        segments,
        init,
        target_duration: m.target_duration as u32,
        total_duration,
    }))
}

fn parse_key(k: &m3u8_rs::Key, base: Option<&Url>) -> Result<Encryption> {
    match k.method {
        m3u8_rs::KeyMethod::None => Ok(Encryption::None),
        m3u8_rs::KeyMethod::AES128 => {
            let uri_str = k
                .uri
                .as_deref()
                .ok_or_else(|| DownloadError::Parse("AES-128 key without URI".into()))?;
            let key_uri = resolve_url(uri_str, base)?;
            let iv = parse_iv(k.iv.as_deref())?;
            Ok(Encryption::Aes128Cbc { key_uri, iv })
        }
        m3u8_rs::KeyMethod::SampleAES => Err(DownloadError::Unsupported {
            feature: "SAMPLE-AES (FairPlay) — see docs/SCOPE.md".into(),
        }),
        m3u8_rs::KeyMethod::Other(ref m) => Err(DownloadError::Unsupported {
            feature: format!("encryption method {m} — see docs/SCOPE.md"),
        }),
    }
}

/// Parse `#EXT-X-KEY:IV=0xHEX...` into 16 bytes. Spec allows omitted IV (uses media sequence
/// number); for Tier 1 we only support explicit IV — segments without IV → Parse error.
fn parse_iv(iv_str: Option<&str>) -> Result<[u8; 16]> {
    let s = iv_str.ok_or_else(|| {
        DownloadError::Parse("AES-128 without explicit IV (media-seq fallback not yet implemented)".into())
    })?;
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    if s.len() != 32 {
        return Err(DownloadError::Parse(format!("IV length {} != 32 hex chars", s.len())));
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .map_err(|e| DownloadError::Parse(format!("IV hex parse: {e}")))?;
    }
    Ok(out)
}

fn resolve_url(s: &str, base: Option<&Url>) -> Result<Url> {
    if let Ok(u) = Url::parse(s) {
        return Ok(u);
    }
    let base = base
        .ok_or_else(|| DownloadError::Parse(format!("relative URL '{s}' but no base supplied")))?;
    base.join(s).map_err(|e| DownloadError::Parse(format!("URL join '{s}': {e}")))
}
