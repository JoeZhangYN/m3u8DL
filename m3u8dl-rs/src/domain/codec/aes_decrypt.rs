//! AES-128-CBC decryption with PKCS7 padding (HLS standard).
//!
//! HLS spec (RFC 8216 §6.2.4): segments encrypted with `METHOD=AES-128` use
//! AES-128-CBC with PKCS7 padding. IV either comes from `#EXT-X-KEY:IV=0xHEX`
//! or — when absent — from the segment's media sequence number (we reject the
//! latter for now; see `parser::parse_iv`).
//!
//! `decrypt(ciphertext, key, iv)` returns plaintext bytes. The whole segment is
//! decrypted in memory (HLS segments are typically a few MB; streaming chunked
//! decryption would buy little).

use aes::Aes128;
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};

use crate::domain::{DownloadError, Result};

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// Decrypt one HLS segment. `ciphertext.len()` must be a multiple of 16 (AES block size).
pub fn decrypt(ciphertext: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>> {
    if ciphertext.is_empty() {
        return Err(DownloadError::Decrypt("empty ciphertext".into()));
    }
    if !ciphertext.len().is_multiple_of(16) {
        return Err(DownloadError::Decrypt(format!(
            "ciphertext length {} not a multiple of 16",
            ciphertext.len()
        )));
    }
    let dec = Aes128CbcDec::new(key.into(), iv.into());
    dec.decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|e| DownloadError::Decrypt(format!("AES-128-CBC unpad: {e}")))
}
