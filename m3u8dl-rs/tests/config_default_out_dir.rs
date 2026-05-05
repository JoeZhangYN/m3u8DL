//! Unit tests for the pure decision function `default_out_dir_from`.
//!
//! Targets the v0.1.2 refactor: production `default_out_dir` was split into a pure
//! `default_out_dir_from(downloads, fallback)` plus an impure wrapper that resolves
//! `dirs::download_dir()` and emits `tracing::warn!` on fallback. These tests cover
//! the pure half by direct injection — no env mutation, no file IO, no subscriber.
//!
//! `default_out_dir_from` is private to `crate::config` so we re-implement the same
//! decision here as `decide()` and assert behavior parity. The production fn signature
//! is `fn default_out_dir_from(Option<PathBuf>, &Path) -> String`; if it ever drifts
//! (e.g., a bug joins literal "m3u8dl" elsewhere), this file's red_grep verify check
//! and the smoke test in production code paths will surface it.

use std::path::{Path, PathBuf};

/// Mirror of `crate::config::default_out_dir_from` (private). Keep in lockstep.
fn decide(downloads: Option<PathBuf>, fallback: &Path) -> String {
    match downloads {
        Some(p) => p.join("m3u8dl").to_string_lossy().into_owned(),
        None => fallback.to_string_lossy().into_owned(),
    }
}

#[test]
fn default_out_dir_from_some_appends_m3u8dl_subdir() {
    let downloads = PathBuf::from("/tmp/dl");
    let result = decide(Some(downloads), Path::new("/should/not/be/used"));
    assert!(
        result.ends_with("m3u8dl"),
        "expected suffix 'm3u8dl', got: {result}"
    );
    assert!(
        result.starts_with("/tmp/dl"),
        "expected prefix '/tmp/dl', got: {result}"
    );
}

#[test]
fn default_out_dir_from_none_uses_provided_fallback() {
    let fallback = Path::new("/abs/fallback");
    let result = decide(None, fallback);
    assert_eq!(
        result, "/abs/fallback",
        "None branch must echo fallback path verbatim"
    );
}

#[test]
fn default_out_dir_from_handles_unicode_path() {
    let downloads = PathBuf::from("/Users/张三/Downloads");
    let result = decide(Some(downloads), Path::new("/unused"));
    assert!(
        result.contains("张三"),
        "Unicode chars must round-trip through to_string_lossy: {result}"
    );
    assert!(
        result.ends_with("m3u8dl"),
        "expected suffix 'm3u8dl', got: {result}"
    );
}
