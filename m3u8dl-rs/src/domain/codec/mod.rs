//! Pure encoding/decoding helpers — no IO, no external state. Live in `domain/` because
//! they're business-spec algorithms (HLS m3u8 normalization / AES-128-CBC decrypt /
//! PNG envelope stripping for anti-hotlink workarounds), not adapters to a system.

pub mod aes_decrypt;
pub mod m3u8_normalize;
pub mod png_strip;
