use url::Url;

use super::SegmentIndex;

/// Top-level playlist sum type. Either a master (variant selector) or a media (segment list).
/// Discriminated union prevents downstream code from accidentally accessing the wrong shape.
#[derive(Debug, Clone)]
pub enum Playlist {
    Master { variants: Vec<Variant> },
    Media(MediaPlaylist),
}

#[derive(Debug, Clone)]
pub struct MediaPlaylist {
    pub segments: Vec<Segment>,
    pub init: Option<InitSegment>,
    pub target_duration: u32,
    pub total_duration: f64,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub uri: Url,
    pub bandwidth: u64,
    pub resolution: Option<(u32, u32)>,
    pub codecs: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub idx: SegmentIndex,
    pub url: Url,
    pub byte_range: Option<ByteRange>,
    pub duration: f32,
    pub encryption: Encryption,
}

#[derive(Debug, Clone)]
pub struct InitSegment {
    pub url: Url,
    pub byte_range: Option<ByteRange>,
}

#[derive(Debug, Clone, Copy)]
pub struct ByteRange {
    pub length: u64,
    pub offset: Option<u64>,
}

/// Encryption descriptor — `Aes128Cbc` is the only supported encrypted method.
/// Parser returns `Err(Unsupported)` for SAMPLE-AES / Widevine / PlayReady (see docs/SCOPE.md).
///
/// `key_uri` is left unresolved here; `application::download_job` fetches+caches the actual key bytes.
#[derive(Debug, Clone)]
pub enum Encryption {
    None,
    Aes128Cbc { key_uri: Url, iv: [u8; 16] },
}

impl Playlist {
    /// Pick the highest-bandwidth variant from a master, ignoring resolution.
    /// Returns `None` if this is a media playlist (caller already has segments).
    pub fn pick_best_variant(&self) -> Option<&Variant> {
        match self {
            Playlist::Master { variants } => variants.iter().max_by_key(|v| v.bandwidth),
            Playlist::Media(_) => None,
        }
    }
}
