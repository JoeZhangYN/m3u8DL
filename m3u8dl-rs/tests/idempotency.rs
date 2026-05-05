//! Unit tests for the IdempotencyTable + IdempotencyKey types. D1a verifies the module
//! in isolation before D1b wires it into routes.rs.

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::time::{Duration, Instant};

use m3u8dl_server::application::idempotency::{
    IdempotencyEntry, IdempotencyKey, IdempotencyKeyError, IdempotencyTable,
};
use m3u8dl_server::domain::JobId;

// ---- IdempotencyKey::try_from validation ----

#[test]
fn try_from_rejects_empty() {
    let err = IdempotencyKey::try_from("").expect_err("empty");
    assert!(matches!(err, IdempotencyKeyError::Empty));
}

#[test]
fn try_from_rejects_too_short() {
    let err = IdempotencyKey::try_from("abc").expect_err("short");
    assert!(matches!(err, IdempotencyKeyError::TooShort(3)));
}

#[test]
fn try_from_rejects_too_long() {
    let s = "a".repeat(129);
    let err = IdempotencyKey::try_from(s).expect_err("long");
    assert!(matches!(err, IdempotencyKeyError::TooLong(129)));
}

#[test]
fn try_from_accepts_minimum_length() {
    let key = IdempotencyKey::try_from("12345678").expect("8 chars ok");
    assert_eq!(key.as_str(), "12345678");
}

#[test]
fn try_from_accepts_maximum_length() {
    let s = "a".repeat(128);
    let key = IdempotencyKey::try_from(s.clone()).expect("128 chars ok");
    assert_eq!(key.as_str().len(), 128);
}

// ---- IdempotencyKey::derive_default determinism ----

#[test]
fn derive_default_is_deterministic_for_same_input() {
    let url = "https://cdn.example.com/v/index.m3u8";
    let mut h = HashMap::new();
    h.insert("Origin".into(), "https://example.com".into());
    h.insert("Referer".into(), "https://example.com/play".into());

    let k1 = IdempotencyKey::derive_default(url, &h);
    let k2 = IdempotencyKey::derive_default(url, &h);
    assert_eq!(k1, k2);
    assert_eq!(k1.as_str().len(), 16); // 8 bytes truncated, 16 hex chars
}

#[test]
fn derive_default_independent_of_header_iteration_order() {
    let url = "https://cdn.example.com/v/index.m3u8";
    let mut h1 = HashMap::new();
    h1.insert("Origin".into(), "https://e.com".into());
    h1.insert("Referer".into(), "https://e.com/p".into());
    h1.insert("Cookie".into(), "sid=abc".into());
    // Different insert order → same hash because derive_default sorts internally
    let mut h2 = HashMap::new();
    h2.insert("Cookie".into(), "sid=abc".into());
    h2.insert("Referer".into(), "https://e.com/p".into());
    h2.insert("Origin".into(), "https://e.com".into());

    let k1 = IdempotencyKey::derive_default(url, &h1);
    let k2 = IdempotencyKey::derive_default(url, &h2);
    assert_eq!(k1, k2, "header order changed key");
}

#[test]
fn derive_default_different_url_yields_different_key() {
    let h: HashMap<String, String> = HashMap::new();
    let k1 = IdempotencyKey::derive_default("https://a.com/x.m3u8", &h);
    let k2 = IdempotencyKey::derive_default("https://b.com/x.m3u8", &h);
    assert_ne!(k1, k2);
}

#[test]
fn derive_default_different_header_value_yields_different_key() {
    let url = "https://x.com/y.m3u8";
    let mut h1 = HashMap::new();
    h1.insert("Cookie".into(), "sid=abc".into());
    let mut h2 = HashMap::new();
    h2.insert("Cookie".into(), "sid=def".into());
    let k1 = IdempotencyKey::derive_default(url, &h1);
    let k2 = IdempotencyKey::derive_default(url, &h2);
    assert_ne!(k1, k2);
}

// ---- IdempotencyTable insert/lookup_fresh ----

#[test]
fn lookup_fresh_returns_entry_within_ttl() {
    let table = IdempotencyTable::new();
    let key = IdempotencyKey::try_from("12345678").expect("ok");
    let job_id = JobId::new();
    table.insert(
        key.clone(),
        IdempotencyEntry::new(job_id.clone(), "title".into()),
    );

    let result = table.lookup_fresh(&key, Instant::now(), Duration::from_secs(300));
    let entry = result.expect("fresh hit");
    assert_eq!(entry.job_id.as_str(), job_id.as_str());
    assert_eq!(entry.title, "title");
}

#[test]
fn lookup_fresh_returns_none_after_ttl() {
    let table = IdempotencyTable::new();
    let key = IdempotencyKey::try_from("12345678").expect("ok");
    let mut entry = IdempotencyEntry::new(JobId::new(), "x".into());
    // Backdate insert to past TTL
    entry.inserted_at = Instant::now() - Duration::from_secs(600);
    table.insert(key.clone(), entry);

    let result = table.lookup_fresh(&key, Instant::now(), Duration::from_secs(300));
    assert!(result.is_none(), "expected None after TTL");
    // Lazy eviction should have removed the entry
    assert_eq!(table.len(), 0, "expired entry not lazily evicted");
}

#[test]
fn lookup_fresh_returns_none_for_unknown_key() {
    let table = IdempotencyTable::new();
    let key = IdempotencyKey::try_from("notthere1").expect("ok");
    let result = table.lookup_fresh(&key, Instant::now(), Duration::from_secs(300));
    assert!(result.is_none());
}

// ---- sweep_expired count ----

#[test]
fn sweep_expired_removes_expired_returns_count() {
    let table = IdempotencyTable::new();
    let now = Instant::now();
    let ttl = Duration::from_secs(300);

    // 3 expired
    for i in 0..3 {
        let key = IdempotencyKey::try_from(format!("expired-{i}")).expect("ok");
        let mut entry = IdempotencyEntry::new(JobId::new(), format!("e{i}"));
        entry.inserted_at = now - Duration::from_secs(600);
        table.insert(key, entry);
    }
    // 2 fresh
    for i in 0..2 {
        let key = IdempotencyKey::try_from(format!("freshxxx-{i}")).expect("ok");
        table.insert(key, IdempotencyEntry::new(JobId::new(), format!("f{i}")));
    }

    assert_eq!(table.len(), 5);
    let removed = table.sweep_expired(now, ttl);
    assert_eq!(removed, 3);
    assert_eq!(table.len(), 2);
}

#[test]
fn sweep_expired_empty_table_returns_zero() {
    let table = IdempotencyTable::new();
    let n = table.sweep_expired(Instant::now(), Duration::from_secs(300));
    assert_eq!(n, 0);
}
