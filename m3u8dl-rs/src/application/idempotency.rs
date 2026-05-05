//! HTTP request-level idempotency: if `POST /download` arrives twice within TTL with the
//! same client-supplied (or server-derived) `Idempotency-Key`, the server returns the
//! existing JobId instead of starting a duplicate download. SOT for the default-key
//! derivation formula and the TTL semantics lives here.
//!
//! Storage is in-memory (`Arc<DashMap>`) — server restart clears the table; clients should
//! be prepared to re-submit. The 5-minute default TTL keeps memory bounded; periodic_sweep
//! (cleanup.rs) belt-and-braces the lazy expiration done by `lookup_fresh`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::domain::JobId;

/// Validated idempotency key. Length-bounded to prevent abuse (clients can't insert 1MB keys
/// to OOM the server). 8..=128 chars is wide enough for any reasonable hash hex / UUID / nonce.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct IdempotencyKey(String);

#[derive(Debug, Error)]
pub enum IdempotencyKeyError {
    #[error("idempotency_key empty")]
    Empty,
    #[error("idempotency_key too short ({0} < 8)")]
    TooShort(usize),
    #[error("idempotency_key too long ({0} > 128)")]
    TooLong(usize),
}

impl IdempotencyKey {
    /// Validate a client-supplied key. Length 8..=128 chars; non-empty.
    pub fn try_from(s: impl Into<String>) -> Result<Self, IdempotencyKeyError> {
        let s = s.into();
        if s.is_empty() {
            return Err(IdempotencyKeyError::Empty);
        }
        if s.len() < 8 {
            return Err(IdempotencyKeyError::TooShort(s.len()));
        }
        if s.len() > 128 {
            return Err(IdempotencyKeyError::TooLong(s.len()));
        }
        Ok(Self(s))
    }

    /// SOT for default key derivation when client doesn't supply one. `sha256(url +
    /// sorted_kv_headers)` truncated to 16 hex chars (64 bits = ~10^19 collision space,
    /// fine for 5-min TTL window). Sorting headers ensures map-iteration order doesn't
    /// affect the key.
    pub fn derive_default(url: &str, headers: &HashMap<String, String>) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let mut entries: Vec<_> = headers.iter().collect();
        entries.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in entries {
            hasher.update(b"\x00");
            hasher.update(k.as_bytes());
            hasher.update(b"=");
            hasher.update(v.as_bytes());
        }
        let digest = hasher.finalize();
        let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
        Self(hex)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stored entry — links a key to the job it created and the time it was inserted (for TTL).
#[derive(Debug, Clone)]
pub struct IdempotencyEntry {
    pub job_id: JobId,
    pub title: String,
    pub inserted_at: Instant,
}

impl IdempotencyEntry {
    pub fn new(job_id: JobId, title: String) -> Self {
        Self {
            job_id,
            title,
            inserted_at: Instant::now(),
        }
    }

    /// Time-since-insert ≥ ttl ⇒ expired.
    pub fn is_expired(&self, now: Instant, ttl: Duration) -> bool {
        now.saturating_duration_since(self.inserted_at) >= ttl
    }
}

/// In-memory key→entry table. Clone-cheap (Arc<DashMap>); one per server.
#[derive(Clone, Default)]
pub struct IdempotencyTable {
    inner: Arc<DashMap<IdempotencyKey, IdempotencyEntry>>,
}

impl IdempotencyTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, key: IdempotencyKey, entry: IdempotencyEntry) {
        self.inner.insert(key, entry);
    }

    /// Look up a key; return `Some(entry)` only if not expired. Lazily evicts on hit
    /// when expired (so a stale key doesn't linger).
    pub fn lookup_fresh(
        &self,
        key: &IdempotencyKey,
        now: Instant,
        ttl: Duration,
    ) -> Option<IdempotencyEntry> {
        let r = self.inner.get(key)?;
        if r.is_expired(now, ttl) {
            drop(r);
            self.inner.remove(key);
            None
        } else {
            Some(r.clone())
        }
    }

    /// Drop all expired entries; return count removed. Periodic sweep companion to
    /// the lazy eviction in `lookup_fresh`.
    pub fn sweep_expired(&self, now: Instant, ttl: Duration) -> usize {
        let expired: Vec<IdempotencyKey> = self
            .inner
            .iter()
            .filter(|r| r.value().is_expired(now, ttl))
            .map(|r| r.key().clone())
            .collect();
        let count = expired.len();
        for k in expired {
            self.inner.remove(&k);
        }
        count
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}
