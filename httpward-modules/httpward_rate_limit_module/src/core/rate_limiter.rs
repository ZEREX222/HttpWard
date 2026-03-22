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

/// Status of a rate limit check result
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum RateLimitCheckStatus {
    /// Request was allowed - within acceptable limits
    Allow,
    /// Request was declined - limit exceeded
    Declined,
    /// New entry - first request, added to limits
    New,
}

/// Detailed result of a single rate limit check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitCheckResult {
    /// Type of rate limit key used (IP, JA4, HeaderFingerprint, Cookie)
    pub kind: RateLimitKeyKind,
    /// Scope of the rule (Global or Route-specific)
    pub scope_type: String, // "Global" or "Route"
    /// The value that was checked
    pub value: String,
    /// Result status of this check
    pub status: RateLimitCheckStatus,
}

/// Collection of all rate limit check results for a request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitCheckResults {
    /// All individual check results
    pub checks: Vec<RateLimitCheckResult>,
    /// Overall result - true if all checks passed (Allow or New)
    pub allowed: bool,
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
    /// Base cooldown after exceeding the limit.
    /// `Duration::ZERO` = disabled (default behaviour).
    /// Each consecutive violation doubles the cooldown (capped at 1 hour).
    pub cooldown: Duration,
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
            cooldown: self.cooldown, // Duration::ZERO = disabled
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
    /// When the active cooldown period ends (`None` = no cooldown).
    cooldown_until: Option<Instant>,
}

impl TokenBucket {
    /// Create new bucket with full capacity
    fn new(capacity: u32) -> Self {
        let now = Instant::now();
        Self {
            tokens: capacity,
            last_refill: now,
            last_seen: now,
            cooldown_until: None,
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

    /// Returns `true` while a cooldown is active.
    fn is_cooling_down(&self) -> bool {
        self.cooldown_until
            .map(|until| Instant::now() < until)
            .unwrap_or(false)
    }

    /// Clear the active cooldown.
    fn reset_cooldown(&mut self) {
        self.cooldown_until = None;
    }

    /// Restore tokens to full capacity and clear any active cooldown.
    fn restore_tokens(&mut self, capacity: u32) {
        self.tokens = capacity;
        self.last_refill = Instant::now();
        self.cooldown_until = None;
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

    /// Attempt to consume one token.
    ///
    /// If `cooldown` is non-zero and all tokens are exhausted, the cooldown timer
    /// is (re)started for exactly `cooldown` duration on every blocked request.
    /// Once the timer expires the bucket refills normally again.
    fn consume(
        &mut self,
        capacity: u32,
        refill_every: Duration,
        refill_amount: u32,
        cooldown: Duration,
    ) -> bool {
        self.touch();

        // Always check cooldown first, but restart timer if this is another violation
        if self.is_cooling_down() {
            // Restart cooldown timer on repeated violations during cooldown period
            if self.tokens == 0 && !cooldown.is_zero() {
                self.cooldown_until = Some(Instant::now() + cooldown);
            }
            return false;
        }

        self.refill(capacity, refill_every, refill_amount);

        if self.tokens == 0 {
            // Start cooldown timer on first violation
            if !cooldown.is_zero() {
                self.cooldown_until = Some(Instant::now() + cooldown);
            }
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

    /// Check rate limit for a single key and return status (Allow, Declined, or New)
    fn check_key_with_status(&mut self, key: &RateKey) -> RateLimitCheckStatus {
        let Some(rule) = self.rules.get(&key.rule).copied() else {
            // No rule defined, allow by default
            return RateLimitCheckStatus::Allow;
        };

        // Check if bucket exists
        if let Some(bucket) = self.buckets.get_mut(key) {
            // If expired, recreate
            if bucket.expired(self.idle_ttl) {
                let mut new_bucket = TokenBucket::new(rule.capacity);
                let allowed = new_bucket.consume(
                    rule.capacity,
                    rule.refill_every,
                    rule.refill_amount,
                    rule.cooldown,
                );
                self.buckets.insert(key.clone(), new_bucket);
                return if allowed {
                    RateLimitCheckStatus::Allow
                } else {
                    RateLimitCheckStatus::Declined
                };
            }

            let allowed = bucket.consume(
                rule.capacity,
                rule.refill_every,
                rule.refill_amount,
                rule.cooldown,
            );
            return if allowed {
                RateLimitCheckStatus::Allow
            } else {
                RateLimitCheckStatus::Declined
            };
        }

        // Create new bucket
        if self.buckets.len() >= self.max_entries {
            // Remove oldest expired bucket or least recently used
            self.evict_one();
        }

        let mut bucket = TokenBucket::new(rule.capacity);
        let allowed = bucket.consume(
            rule.capacity,
            rule.refill_every,
            rule.refill_amount,
            rule.cooldown,
        );
        self.buckets.insert(key.clone(), bucket);

        if allowed {
            RateLimitCheckStatus::New
        } else {
            RateLimitCheckStatus::Declined
        }
    }

    /// Check rate limit for a single key (simplified, returns bool)
    fn check_key(&mut self, key: &RateKey) -> bool {
        match self.check_key_with_status(key) {
            RateLimitCheckStatus::Allow | RateLimitCheckStatus::New => true,
            RateLimitCheckStatus::Declined => false,
        }
    }

    /// Check rate limit by components
    pub fn check(&mut self, kind: RateLimitKeyKind, scope: RateLimitScope, value: &str) -> bool {
        self.maybe_cleanup();
        let key = self.make_key(kind, scope, value);
        self.check_key(&key)
    }

    /// Check rate limit and return detailed results for each check
    pub fn check_with_results(
        &mut self,
        checks: &[(RateLimitKeyKind, RateLimitScope, String)],
    ) -> RateLimitCheckResults {
        self.maybe_cleanup();

        let mut results = Vec::new();
        let mut all_allowed = true;

        for (kind, scope, value) in checks {
            let key = self.make_key(kind.clone(), scope.clone(), value);
            let status = self.check_key_with_status(&key);

            let scope_type = match scope {
                RateLimitScope::Global => "Global".to_string(),
                RateLimitScope::Route(_) => "Route".to_string(),
            };

            results.push(RateLimitCheckResult {
                kind: kind.clone(),
                scope_type,
                value: value.clone(),
                status: status.clone(),
            });

            // Overall result is false only if any check is Declined
            if status == RateLimitCheckStatus::Declined {
                all_allowed = false;
            }
        }

        RateLimitCheckResults {
            checks: results,
            allowed: all_allowed,
        }
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

    /// Reset the cooldown for a specific client value (e.g. an IP address or
    /// JA4 fingerprint string).
    ///
    /// The `value` must be the same raw string that is passed to `check` /
    /// `check_all` on every request so the bucket key matches.
    pub fn reset_cooldown(&mut self, kind: RateLimitKeyKind, scope: RateLimitScope, value: &str) {
        let key = self.make_key(kind, scope, value);
        if let Some(bucket) = self.buckets.get_mut(&key) {
            bucket.reset_cooldown();
        }
    }

    /// Reset cooldowns for **all** tracked buckets regardless of kind or scope.
    pub fn reset_all_cooldowns(&mut self) {
        for bucket in self.buckets.values_mut() {
            bucket.reset_cooldown();
        }
    }

    /// Restore tokens to full capacity for a specific client value and clear
    /// any active cooldown on that bucket.
    pub fn restore_tokens(&mut self, kind: RateLimitKeyKind, scope: RateLimitScope, value: &str) {
        let key = self.make_key(kind.clone(), scope.clone(), value);
        let rule_id = RuleId { kind, scope };
        if let (Some(bucket), Some(rule)) = (self.buckets.get_mut(&key), self.rules.get(&rule_id)) {
            bucket.restore_tokens(rule.capacity);
        }
    }

    /// Restore tokens to full capacity for **all** tracked buckets and clear
    /// every active cooldown.
    pub fn restore_all_tokens(&mut self) {
        for (key, bucket) in self.buckets.iter_mut() {
            if let Some(rule) = self.rules.get(&key.rule) {
                bucket.restore_tokens(rule.capacity);
            }
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

    fn rule(capacity: u32, refill_every: Duration, refill_amount: u32) -> RateLimitRule {
        RateLimitRule {
            capacity,
            refill_every,
            refill_amount,
            cooldown: Duration::ZERO,
        }
    }

    #[test]
    fn test_token_bucket_basic() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            rule(5, Duration::from_secs(1), 1),
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
            rule(3, Duration::from_secs(1), 1),
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
            rule(1, Duration::from_millis(40), 1),
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
            rule(2, Duration::from_secs(1), 1),
        );

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.2"));
        assert_eq!(limiter.bucket_count(), 1);

        thread::sleep(Duration::from_millis(30));
        limiter.cleanup();

        assert_eq!(limiter.bucket_count(), 0);
    }

    #[test]
    fn test_cooldown_blocks_after_limit() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 2,
                refill_every: Duration::from_millis(1),
                refill_amount: 1,
                cooldown: Duration::from_secs(60), // long cooldown for test
            },
        );

        // Exhaust the bucket
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.2.3.4"));
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.2.3.4"));
        // Limit hit — cooldown activated
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.2.3.4"));

        // Even after a short sleep (tokens would refill without cooldown), still blocked
        thread::sleep(Duration::from_millis(10));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.2.3.4"));
    }

    #[test]
    fn test_cooldown_zero_does_not_block() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 1,
                refill_every: Duration::from_millis(5),
                refill_amount: 1,
                cooldown: Duration::ZERO, // disabled
            },
        );

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "5.6.7.8"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "5.6.7.8"));

        // Token refills → allowed again (no cooldown holding it back)
        thread::sleep(Duration::from_millis(10));
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "5.6.7.8"));
    }

    // ── reset_cooldown ────────────────────────────────────────────────────────

    /// Basic: cooldown is cleared and the client can make requests again.
    #[test]
    fn test_reset_cooldown_unblocks_client() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 1,
                refill_every: Duration::from_millis(1),
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "9.9.9.9"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "9.9.9.9")); // cooldown active

        limiter.reset_cooldown(RateLimitKeyKind::Ip, RateLimitScope::Global, "9.9.9.9");

        thread::sleep(Duration::from_millis(5)); // let token refill
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "9.9.9.9"));
    }

    /// Cooldown timer restarts on repeated violations during cooldown period.
    /// This prevents clients from "waiting out" the cooldown by making repeated requests.
    #[test]
    fn test_cooldown_restarts_on_repeated_violations() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 1,
                refill_every: Duration::from_millis(1),
                refill_amount: 1,
                cooldown: Duration::from_secs(2), // 2 seconds for fast test
            },
        );

        // 1. Exhaust the token - cooldown starts
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1")); // cooldown activated

        // 2. Wait 1 second (half of cooldown period)
        thread::sleep(Duration::from_secs(1));
        
        // 3. Make another request during cooldown - this should restart the timer
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1")); // cooldown restarted

        // 4. Wait another 1 second (would have been enough for original cooldown)
        thread::sleep(Duration::from_secs(1));
        
        // 5. Should still be blocked because cooldown was restarted
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));

        // 6. Wait full 2 seconds after the restart
        thread::sleep(Duration::from_secs(2));
        
        // 7. Now should be allowed
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
    }

    /// Cooldown timer restarts multiple times on multiple violations during cooldown.
    #[test]
    fn test_cooldown_restarts_multiple_times() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 1,
                refill_every: Duration::from_millis(1),
                refill_amount: 1,
                cooldown: Duration::from_millis(500), // 0.5 seconds for fast test
            },
        );

        // Exhaust token
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1")); // cooldown starts

        // Make multiple requests during cooldown, each should restart the timer
        for i in 0..5 {
            thread::sleep(Duration::from_millis(100)); // Wait 100ms
            assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"), 
                    "Request {} should be blocked (cooldown restarted)", i + 1);
        }

        // After 5 restarts, we need to wait full cooldown period from the last restart
        thread::sleep(Duration::from_millis(500));
        
        // Now should be allowed
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));
    }

    /// reset_cooldown must NOT restore tokens — only the timer is cleared.
    #[test]
    fn test_reset_cooldown_does_not_restore_tokens() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 3,
                refill_every: Duration::from_secs(3600), // very slow refill
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        // Exhaust all tokens → cooldown kicks in
        for _ in 0..3 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.2.3.4"));
        }
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.2.3.4"));

        // Clear only the cooldown
        limiter.reset_cooldown(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.2.3.4");

        // Tokens are still 0 — request is still denied (no refill yet)
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.2.3.4"));
    }

    /// reset_cooldown on a bucket with no active cooldown is a safe no-op.
    #[test]
    fn test_reset_cooldown_noop_when_no_cooldown_active() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 5,
                refill_every: Duration::from_millis(1),
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "2.2.2.2"));

        // No cooldown is active — call should not panic or break anything
        limiter.reset_cooldown(RateLimitKeyKind::Ip, RateLimitScope::Global, "2.2.2.2");

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "2.2.2.2"));
    }

    /// reset_cooldown for an unknown value (no bucket yet) is a safe no-op.
    #[test]
    fn test_reset_cooldown_noop_for_unknown_value() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 2,
                refill_every: Duration::from_millis(1),
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        // "ghost" IP — never seen before
        limiter.reset_cooldown(RateLimitKeyKind::Ip, RateLimitScope::Global, "0.0.0.0");

        // Normal operation unaffected
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "0.0.0.0"));
    }

    /// reset_cooldown on one client must not affect another client's cooldown.
    #[test]
    fn test_reset_cooldown_only_affects_target_client() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 1,
                refill_every: Duration::from_secs(3600),
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        // Both clients exhaust their buckets
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.2"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.2"));

        // Reset only .1
        limiter.reset_cooldown(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1");

        // .2 still blocked
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.2"));
    }

    /// reset_all_cooldowns lifts every active cooldown at once.
    #[test]
    fn test_reset_all_cooldowns() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 1,
                refill_every: Duration::from_millis(1),
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.2"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.2"));

        limiter.reset_all_cooldowns();

        thread::sleep(Duration::from_millis(5));
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.1"));
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "10.0.0.2"));
    }

    /// reset_all_cooldowns on a limiter with no active cooldowns is a safe no-op.
    #[test]
    fn test_reset_all_cooldowns_noop_when_no_cooldowns() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            rule(5, Duration::from_millis(1), 1),
        );

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.1.1.1"));

        // No cooldown configured — should not panic
        limiter.reset_all_cooldowns();

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.1.1.1"));
    }

    // ── restore_tokens ───────────────────────────────────────────────────────

    /// Basic: tokens are restored to capacity AND cooldown is cleared.
    #[test]
    fn test_restore_tokens_refills_and_clears_cooldown() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 3,
                refill_every: Duration::from_secs(60),
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        // Exhaust bucket — cooldown activates
        for _ in 0..3 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.1.1.1"));
        }
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.1.1.1"));

        limiter.restore_tokens(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.1.1.1");

        // All 3 tokens available immediately — no sleep needed
        for _ in 0..3 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "1.1.1.1"));
        }
    }

    /// restore_tokens refills a *partially* consumed bucket back to full capacity.
    #[test]
    fn test_restore_tokens_refills_partial_bucket() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 5,
                refill_every: Duration::from_secs(3600),
                refill_amount: 1,
                cooldown: Duration::ZERO,
            },
        );

        // Consume 3 tokens
        for _ in 0..3 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "4.4.4.4"));
        }

        // Restore to full
        limiter.restore_tokens(RateLimitKeyKind::Ip, RateLimitScope::Global, "4.4.4.4");

        // All 5 tokens available again
        for _ in 0..5 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "4.4.4.4"));
        }
        // 6th denied
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "4.4.4.4"));
    }

    /// restore_tokens also clears a cooldown even when there was no token exhaustion
    /// (e.g. a manually injected cooldown via some future mechanism).
    /// Here we verify it via the normal path: exhaust → restore → full capacity.
    #[test]
    fn test_restore_tokens_clears_cooldown_without_waiting() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 2,
                refill_every: Duration::from_secs(3600), // effectively no refill
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        for _ in 0..2 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "5.5.5.5"));
        }
        // cooldown now active
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "5.5.5.5"));

        limiter.restore_tokens(RateLimitKeyKind::Ip, RateLimitScope::Global, "5.5.5.5");

        // Immediate access — no sleep, no refill window needed
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "5.5.5.5"));
    }

    /// restore_tokens on a non-existent bucket is a safe no-op (no panic, no insertion).
    #[test]
    fn test_restore_tokens_noop_for_unknown_value() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            rule(3, Duration::from_millis(1), 1),
        );

        let before = limiter.bucket_count();

        // Should not create a bucket or panic
        limiter.restore_tokens(RateLimitKeyKind::Ip, RateLimitScope::Global, "0.0.0.0");

        assert_eq!(limiter.bucket_count(), before);
    }

    /// restore_tokens on one client must not affect another client's bucket.
    #[test]
    fn test_restore_tokens_only_affects_target_client() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 1,
                refill_every: Duration::from_secs(3600),
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        // Exhaust both clients
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "6.6.6.6"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "6.6.6.6"));
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "7.7.7.7"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "7.7.7.7"));

        // Restore only .6
        limiter.restore_tokens(RateLimitKeyKind::Ip, RateLimitScope::Global, "6.6.6.6");

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "6.6.6.6")); // restored
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "7.7.7.7")); // still blocked
    }

    /// restore_all_tokens restores every tracked bucket to full capacity.
    #[test]
    fn test_restore_all_tokens() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 1,
                refill_every: Duration::from_secs(60),
                refill_amount: 1,
                cooldown: Duration::from_secs(3600),
            },
        );

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "2.2.2.2"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "2.2.2.2"));
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "3.3.3.3"));
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "3.3.3.3"));

        limiter.restore_all_tokens();

        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "2.2.2.2"));
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "3.3.3.3"));
    }

    /// restore_all_tokens when no buckets exist is a safe no-op.
    #[test]
    fn test_restore_all_tokens_noop_when_empty() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            rule(5, Duration::from_millis(1), 1),
        );

        assert_eq!(limiter.bucket_count(), 0);
        limiter.restore_all_tokens(); // must not panic
        assert_eq!(limiter.bucket_count(), 0);
    }

    /// After restore_all_tokens each bucket allows exactly `capacity` requests
    /// before hitting the limit again.
    #[test]
    fn test_restore_all_tokens_respects_capacity() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            RateLimitRule {
                capacity: 3,
                refill_every: Duration::from_secs(3600),
                refill_amount: 1,
                cooldown: Duration::ZERO,
            },
        );

        // Use up all 3 tokens
        for _ in 0..3 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "8.8.8.8"));
        }
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "8.8.8.8"));

        limiter.restore_all_tokens();

        // Exactly 3 tokens again — not more
        for _ in 0..3 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "8.8.8.8"));
        }
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "8.8.8.8"));
    }
}
