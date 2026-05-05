//! Property tests for `aes_decrypt::decrypt`. The hand-written cases in `aes_decrypt.rs`
//! cover specific lengths (short / 64 / 256KB) — these widen the input distribution to
//! catch padding / length / key-IV-coupling regressions that targeted cases would miss.
//!
//! `Config::with_cases(64)` keeps CI runtime bounded; bump locally with `PROPTEST_CASES`.

use aes::Aes128;
use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use m3u8dl_server::domain::codec::aes_decrypt::decrypt;
use proptest::prelude::*;

type Aes128CbcEnc = cbc::Encryptor<Aes128>;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// For any plaintext length 0..=4096, any 16-byte key/iv:
    /// encrypt(p, k, iv) → decrypt(ct, k, iv) must equal p.
    #[test]
    fn encrypt_decrypt_roundtrip(
        plain in proptest::collection::vec(any::<u8>(), 0..=4096),
        key in any::<[u8; 16]>(),
        iv in any::<[u8; 16]>(),
    ) {
        let enc = Aes128CbcEnc::new(&key.into(), &iv.into());
        let ct = enc.encrypt_padded_vec_mut::<Pkcs7>(&plain);
        prop_assert!(ct.len().is_multiple_of(16), "ct not block-aligned: {}", ct.len());

        let pt = decrypt(&ct, &key, &iv).expect("decrypt");
        prop_assert_eq!(pt.as_slice(), plain.as_slice());
    }

    /// PKCS7 ciphertext length invariant: ct.len() == p.len() + (16 - p.len() % 16).
    /// PKCS7 always adds 1..=16 bytes (full block when p.len() % 16 == 0).
    #[test]
    fn pkcs7_ciphertext_length(
        plain in proptest::collection::vec(any::<u8>(), 0..=2048),
    ) {
        let key = [0x42u8; 16];
        let iv = [0x33u8; 16];
        let enc = Aes128CbcEnc::new(&key.into(), &iv.into());
        let ct = enc.encrypt_padded_vec_mut::<Pkcs7>(&plain);

        let pad = 16 - (plain.len() % 16);
        prop_assert_eq!(ct.len(), plain.len() + pad,
            "p.len={} pad={} ct.len={}", plain.len(), pad, ct.len());
    }

    /// Wrong key on valid ciphertext → unpad error (probabilistic but high confidence
    /// over 64 random keys; PKCS7 last-byte must be 1..=16 for unpad to succeed, so
    /// fully-random "decrypt" output unlikely to satisfy that constraint).
    #[test]
    fn wrong_key_fails_unpad(
        plain in proptest::collection::vec(any::<u8>(), 16..=512),
        key in any::<[u8; 16]>(),
        iv in any::<[u8; 16]>(),
        // ensure wrong_key != key by xor'ing in a non-zero byte
        flip_byte in 1u8..=255,
    ) {
        let mut wrong_key = key;
        wrong_key[0] ^= flip_byte;

        let enc = Aes128CbcEnc::new(&key.into(), &iv.into());
        let ct = enc.encrypt_padded_vec_mut::<Pkcs7>(&plain);

        // Wrong key may produce valid-looking padding ~6% of the time (1/16 chance the
        // last byte happens to be in 1..=16); we accept that this property test catches
        // the modal failure mode but not 100%. Hand-written `rejects_wrong_key_with_decrypt_error`
        // covers the deterministic case.
        let result = decrypt(&ct, &wrong_key, &iv);
        if let Ok(pt) = &result {
            prop_assert_ne!(pt.as_slice(), plain.as_slice(),
                "wrong key produced original plaintext");
        }
    }

    /// Non-block-aligned ciphertext (length not a multiple of 16) always rejects.
    #[test]
    fn non_block_aligned_rejects(
        bad_len in (1usize..256).prop_filter("must not be multiple of 16", |n| !n.is_multiple_of(16)),
    ) {
        let key = [0u8; 16];
        let iv = [0u8; 16];
        let bad = vec![0u8; bad_len];
        let err = decrypt(&bad, &key, &iv).expect_err("should reject");
        prop_assert!(err.to_string().contains("multiple of 16"), "got: {err}");
    }
}
