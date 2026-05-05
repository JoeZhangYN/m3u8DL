use m3u8dl_server::domain::codec::png_strip::strip_png_wrapper;

const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const IEND: [u8; 8] = [0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82];

fn build_wrapped(payload: &[u8]) -> Vec<u8> {
    // 8-byte sig + minimal "fake" IHDR-ish chunk (16 bytes filler) + IEND + trailing payload
    let mut v = Vec::new();
    v.extend_from_slice(&PNG_SIG);
    v.extend_from_slice(&[0u8; 16]); // some dummy chunk bytes (algorithm doesn't validate)
    v.extend_from_slice(&IEND);
    v.extend_from_slice(payload);
    v
}

#[test]
fn strips_payload_after_iend() {
    let payload = b"#EXTM3U\n#EXT-X-VERSION:3\nseg0.ts\n";
    let wrapped = build_wrapped(payload);
    let out = strip_png_wrapper(&wrapped);
    assert_eq!(out, payload);
}

#[test]
fn returns_input_when_no_png_signature() {
    let plain = b"#EXTM3U\nseg0.ts\n";
    let out = strip_png_wrapper(plain);
    assert_eq!(out, plain);
}

#[test]
fn returns_input_when_too_short() {
    let tiny = &[0x89, 0x50, 0x4E, 0x47];
    let out = strip_png_wrapper(tiny);
    assert_eq!(out, tiny);
}

#[test]
fn returns_input_when_no_iend_chunk() {
    // PNG sig but no IEND end-marker — treat as real PNG with no trailer
    let mut v = Vec::from(PNG_SIG);
    v.extend_from_slice(&[0u8; 32]);
    let out = strip_png_wrapper(&v);
    assert_eq!(out, v.as_slice());
}

#[test]
fn empty_payload_after_iend_returns_empty() {
    let wrapped = build_wrapped(b"");
    let out = strip_png_wrapper(&wrapped);
    assert!(out.is_empty(), "got {} bytes", out.len());
}
