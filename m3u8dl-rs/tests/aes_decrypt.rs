use aes::Aes128;
use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use hex_literal::hex;

use m3u8dl_server::domain::codec::aes_decrypt::decrypt;

type Aes128CbcEnc = cbc::Encryptor<Aes128>;

#[test]
fn round_trip_short_plaintext() {
    let key: [u8; 16] = hex!("000102030405060708090a0b0c0d0e0f");
    let iv: [u8; 16] = hex!("101112131415161718191a1b1c1d1e1f");
    let plain = b"Hello, AES-128-CBC PKCS7!";

    let enc = Aes128CbcEnc::new(&key.into(), &iv.into());
    let ct = enc.encrypt_padded_vec_mut::<Pkcs7>(plain);
    assert!(
        ct.len().is_multiple_of(16),
        "ciphertext not block-aligned: {}",
        ct.len()
    );

    let pt = decrypt(&ct, &key, &iv).expect("decrypt");
    assert_eq!(pt, plain);
}

#[test]
fn round_trip_exact_block_multiple() {
    // Tests that PKCS7 padding adds a full pad block when plaintext is exactly 16-byte aligned
    let key: [u8; 16] = hex!("0123456789abcdef0123456789abcdef");
    let iv: [u8; 16] = hex!("fedcba9876543210fedcba9876543210");
    let plain = [0x42u8; 64]; // 4 full blocks

    let enc = Aes128CbcEnc::new(&key.into(), &iv.into());
    let ct = enc.encrypt_padded_vec_mut::<Pkcs7>(&plain);
    assert_eq!(ct.len(), 80, "expected 64+16 pad block");

    let pt = decrypt(&ct, &key, &iv).expect("decrypt");
    assert_eq!(pt, plain);
}

#[test]
fn round_trip_large_payload() {
    // Realistic-ish HLS segment size (~256KB)
    let key: [u8; 16] = [0x77; 16];
    let iv: [u8; 16] = [0x33; 16];
    let plain: Vec<u8> = (0..262144u32).map(|i| (i & 0xff) as u8).collect();

    let enc = Aes128CbcEnc::new(&key.into(), &iv.into());
    let ct = enc.encrypt_padded_vec_mut::<Pkcs7>(&plain);

    let pt = decrypt(&ct, &key, &iv).expect("decrypt");
    assert_eq!(pt, plain);
}

#[test]
fn rejects_non_block_aligned_ciphertext() {
    let key = [0u8; 16];
    let iv = [0u8; 16];
    let bad = [0u8; 17];
    let err = decrypt(&bad, &key, &iv).expect_err("should reject");
    assert!(err.to_string().contains("multiple of 16"), "got: {err}");
}

#[test]
fn rejects_empty_ciphertext() {
    let key = [0u8; 16];
    let iv = [0u8; 16];
    let err = decrypt(&[], &key, &iv).expect_err("should reject");
    assert!(err.to_string().contains("empty"), "got: {err}");
}

#[test]
fn rejects_wrong_key_with_decrypt_error() {
    let key: [u8; 16] = [0x11; 16];
    let iv: [u8; 16] = [0x22; 16];
    let plain = b"some plaintext";
    let enc = Aes128CbcEnc::new(&key.into(), &iv.into());
    let ct = enc.encrypt_padded_vec_mut::<Pkcs7>(plain);

    let wrong_key = [0xffu8; 16];
    let err = decrypt(&ct, &wrong_key, &iv).expect_err("should fail unpad");
    assert!(err.to_string().contains("unpad"), "got: {err}");
}
