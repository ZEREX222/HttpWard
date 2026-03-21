use std::collections::HashMap;
/// HttpWard Rate Limiter Core Algorithm
///
/// Token Bucket Algorithm Implementation with:
/// - Per-key token bucket tracking
/// - TTL-based cleanup for memory efficiency
/// - Support for multiple fingerprint types (IP, JA4, Header-based)
/// - Route-scoped and global rate limit rules
/// - Optimized memory management with configurable cache size
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Rate limit key types
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitKeyKind {
    /// IP address based limiting
    Ip,
    /// JA4 TLS fingerprint based limiting
    Ja4,
    /// Header fingerprint based limiting
    HeaderFingerprint,
    /// Cookie/Session based limiting
    Cookie,
}

/// Scope of the rate limit rule
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum RateLimitScope {
    /// Global limit (applies to all routes)
    Global,
    /// Route-specific limit
    Route(RouteScopeKey),
}

/// Stable in-process identity of a matched route.
///
/// This is intentionally compact and cheap to hash/compare.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct RouteScopeKey(usize);

impl RouteScopeKey {
    pub fn from_arc_ptr<T>(value: &std::sync::Arc<T>) -> Self {
        Self(std::sync::Arc::as_ptr(value) as usize)
    }

    pub fn as_usize(self) -> usize {
        self.0
    }
}

/// Unique identifier for a rate limiting rule
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct RuleId {
    kind: RateLimitKeyKind,
    scope: RateLimitScope,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct RateKey {
    rule: RuleId,
    value_hash: u64,
}

/// Configuration for a single rate limiting rule
#[derive(Debug, Clone, Copy)]
pub struct RateLimitRule {
    /// Maximum tokens in bucket (capacity)
    pub capacity: u32,
    /// Duration between refills
    pub refill_every: Duration,
    /// Number of tokens to add per refill period
    pub refill_amount: u32,
}

impl RateLimitRule {
    pub fn sanitized(self) -> Self {
        Self {
            capacity: self.capacity.max(1),
            refill_every: if self.refill_every.is_zero() {
                Duration::from_millis(1)
            } else {
                self.refill_every
            },
            refill_amount: self.refill_amount.max(1),
        }
    }
}

/// Token bucket state
#[derive(Debug, Clone)]
struct TokenBucket {
    /// Current token count
    tokens: u32,
    /// Last refill timestamp
    last_refill: Instant,
    /// Last access timestamp (for TTL)
    last_seen: Instant,
}

impl TokenBucket {
    /// Create new bucket with full capacity
    fn new(capacity: u32) -> Self {
        let now = Instant::now();
        Self {
            tokens: capacity,
            last_refill: now,
            last_seen: now,
        }
    }

    /// Update last access time
    fn touch(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Check if bucket has expired based on TTL
    fn expired(&self, ttl: Duration) -> bool {
        self.last_seen.elapsed() > ttl
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self, capacity: u32, refill_every: Duration, refill_amount: u32) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);

        if elapsed < refill_every || capacity == 0 || refill_amount == 0 {
            return;
        }

        let interval_nanos = refill_every.as_nanos();
        if interval_nanos == 0 {
            self.tokens = capacity;
            self.last_refill = now;
            return;
        }

        let elapsed_nanos = elapsed.as_nanos();
        let steps = elapsed_nanos / interval_nanos;
        if steps == 0 {
            return;
        }

        let add = steps
            .saturating_mul(refill_amount as u128)
            .min(u32::MAX as u128) as u32;

        if add > 0 {
            self.tokens = (self.tokens + add).min(capacity);
            let remainder_nanos = (elapsed_nanos % interval_nanos) as u64;
            self.last_refill = now - Duration::from_nanos(remainder_nanos);
        }
    }

    /// Attempt to consume one token
    fn consume(&mut self, capacity: u32, refill_every: Duration, refill_amount: u32) -> bool {
        self.touch();
        self.refill(capacity, refill_every, refill_amount);

        if self.tokens == 0 {
            return false;
        }

        self.tokens -= 1;
        true
    }
}

/// Hash helper function
fn hash_value(v: &str) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Main rate limiter with in-memory storage
#[derive(Debug)]
pub struct RateLimiter {
    /// Token bucket storage
    buckets: HashMap<RateKey, TokenBucket>,
    /// Rate limit rules
    rules: HashMap<RuleId, RateLimitRule>,
    /// Time-to-live for inactive buckets
    idle_ttl: Duration,
    /// Cleanup cadence.
    cleanup_interval: Duration,
    /// Last opportunistic cleanup run.
    last_cleanup: Instant,
    /// Maximum allowed entries in cache
    max_entries: usize,
}

impl RateLimiter {
    /// Create new rate limiter
    pub fn new(max_entries: usize, idle_ttl: Duration, cleanup_interval: Duration) -> Self {
        Self {
            buckets: HashMap::with_capacity(max_entries.min(1000)),
            rules: HashMap::new(),
            idle_ttl,
            cleanup_interval,
            last_cleanup: Instant::now(),
            max_entries,
        }
    }

    /// Register a new rate limit rule
    pub fn add_rule(&mut self, kind: RateLimitKeyKind, scope: RateLimitScope, rule: RateLimitRule) {
        self.rules.insert(RuleId { kind, scope }, rule.sanitized());
    }

    /// Create a rate key from raw value
    fn make_key(&self, kind: RateLimitKeyKind, scope: RateLimitScope, raw: &str) -> RateKey {
        RateKey {
            rule: RuleId { kind, scope },
            value_hash: hash_value(raw),
        }
    }

    /// Check rate limit for a single key
    fn check_key(&mut self, key: &RateKey) -> bool {
        let Some(rule) = self.rules.get(&key.rule).copied() else {
            // No rule defined, allow by default
            return true;
        };

        // Check if bucket exists
        if let Some(bucket) = self.buckets.get_mut(key) {
            // If expired, recreate
            if bucket.expired(self.idle_ttl) {
                let mut new_bucket = TokenBucket::new(rule.capacity);
                let allowed =
                    new_bucket.consume(rule.capacity, rule.refill_every, rule.refill_amount);
                self.buckets.insert(key.clone(), new_bucket);
                return allowed;
            }

            return bucket.consume(rule.capacity, rule.refill_every, rule.refill_amount);
        }

        // Create new bucket
        if self.buckets.len() >= self.max_entries {
            // Remove oldest expired bucket or least recently used
            self.evict_one();
        }

        let mut bucket = TokenBucket::new(rule.capacity);
        let allowed = bucket.consume(rule.capacity, rule.refill_every, rule.refill_amount);
        self.buckets.insert(key.clone(), bucket);

        allowed
    }

    /// Check rate limit by components
    pub fn check(&mut self, kind: RateLimitKeyKind, scope: RateLimitScope, value: &str) -> bool {
        self.maybe_cleanup();
        let key = self.make_key(kind, scope, value);
        self.check_key(&key)
    }

    /// Cleanup expired buckets
    pub fn cleanup(&mut self) {
        let expired_keys: Vec<_> = self
            .buckets
            .iter()
            .filter(|(_, v)| v.expired(self.idle_ttl))
            .map(|(k, _)| k.clone())
            .collect();

        for key in expired_keys {
            self.buckets.remove(&key);
        }

        self.last_cleanup = Instant::now();
    }

    fn maybe_cleanup(&mut self) {
        if self.buckets.len() >= self.max_entries
            || self.last_cleanup.elapsed() >= self.cleanup_interval
        {
            self.cleanup();
        }
    }

    /// Evict one bucket (either expired or LRU)
    fn evict_one(&mut self) {
        // First try to find and remove an expired bucket
        if let Some(expired_key) = self
            .buckets
            .iter()
            .find(|(_, v)| v.expired(self.idle_ttl))
            .map(|(k, _)| k.clone())
        {
            self.buckets.remove(&expired_key);
            return;
        }

        // If no expired buckets, remove least recently used
        if let Some((key_to_remove, _)) = self
            .buckets
            .iter()
            .min_by_key(|(_, v)| v.last_seen)
            .map(|(k, v)| (k.clone(), v.clone()))
        {
            self.buckets.remove(&key_to_remove);
        }
    }

    /// Get current bucket count
    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    /// Get rule count
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_token_bucket_basic() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 5,
                refill_every: Duration::from_secs(1),
                refill_amount: 1,
            },
        );

        // First 5 requests should pass
        for _ in 0..5 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
        }

        // 6th request should fail
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
    }

    #[test]
    fn test_route_scoped_limit() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Route(RouteScopeKey(1)),
            RateLimitRule {
                capacity: 3,
                refill_every: Duration::from_secs(1),
                refill_amount: 1,
            },
        );

        // Should allow 3 requests to /login
        for _ in 0..3 {
            assert!(limiter.check(
                RateLimitKeyKind::Ip,
                RateLimitScope::Route(RouteScopeKey(1)),
                "192.168.1.1"
            ));
        }

        // Should block 4th request
        assert!(!limiter.check(
            RateLimitKeyKind::Ip,
            RateLimitScope::Route(RouteScopeKey(1)),
            "192.168.1.1"
        ));
    }

    #[test]
    fn test_refill_uses_whole_steps_without_drift() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 1,
                refill_every: Duration::from_millis(40),
                refill_amount: 1,
            },
        );

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));

        thread::sleep(Duration::from_millis(45));

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));
    }

    #[test]
    fn test_cleanup_removes_idle_buckets() {
        let mut limiter =
            RateLimiter::new(100, Duration::from_millis(20), Duration::from_millis(5));

        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 2,
                refill_every: Duration::from_secs(1),
                refill_amount: 1,
            },
        );

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.2"));
        assert_eq!(limiter.bucket_count(), 1);

        thread::sleep(Duration::from_millis(30));
        limiter.cleanup();

        assert_eq!(limiter.bucket_count(), 0);
    }
}
