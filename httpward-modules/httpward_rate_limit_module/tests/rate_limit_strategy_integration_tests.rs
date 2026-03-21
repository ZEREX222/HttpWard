use std::collections::HashMap;
use std::time::Duration;

use httpward_rate_limit_module::{
    HttpWardRateLimitConfig, InternalRateLimitRule, RateLimitKeyKind, RateLimitRuleConfig,
    RateLimitScope, RateLimitStrategy, RateLimiter,
};

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_sliding_strategy_runtime_behavior() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        // Add sliding rule: 50 requests in 10 seconds
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            httpward_rate_limit_module::rate_limiter::RateLimitRule {
                capacity: 50,
                refill_every: Duration::from_millis(200), // 10s / 50 = 200ms
                refill_amount: 1,
            },
        );

        // Should allow 50 requests immediately
        for _ in 0..50 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
        }

        // 51st request should fail
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));

        // Wait for partial refill
        std::thread::sleep(Duration::from_millis(250));

        // Should allow 1 more request
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
    }

    #[test]
    fn test_burst_strategy_runtime_behavior() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        // Add burst rule: 50 requests in 10 seconds (current behavior)
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            httpward_rate_limit_module::rate_limiter::RateLimitRule {
                capacity: 50,
                refill_every: Duration::from_secs(2),
                refill_amount: 1,
            },
        );

        // Should allow 50 requests immediately
        for _ in 0..50 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
        }

        // 51st request should fail
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));

        // Wait for refill - need to wait longer than 10s for burst strategy
        std::thread::sleep(Duration::from_millis(2500));

        // Should allow 1 more request
        assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
    }

    #[test]
    fn test_fixed_strategy_runtime_behavior() {
        let mut limiter = RateLimiter::new(100, Duration::from_secs(60), Duration::from_secs(10));

        // Add fixed rule: 10 requests per hour
        limiter.add_rule(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            httpward_rate_limit_module::rate_limiter::RateLimitRule {
                capacity: 10,
                refill_every: Duration::from_secs(3600),
                refill_amount: 10, // Complete refill
            },
        );

        // Should allow 10 requests immediately
        for _ in 0..10 {
            assert!(limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
        }

        // 11th request should fail
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));

        // Wait for complete refill (simulate with shorter time for test)
        // In real scenario, this would be 1 hour
        std::thread::sleep(Duration::from_millis(100));

        // Still should fail (not enough time passed)
        assert!(!limiter.check(RateLimitKeyKind::Ip, RateLimitScope::Global, "192.168.1.1"));
    }

    #[test]
    fn test_config_to_runtime_rule_conversion() {
        let test_cases = vec![
            (
                RateLimitStrategy::Sliding,
                50,
                Duration::from_secs(10),
                Duration::from_millis(200),
                1,
            ),
            (
                RateLimitStrategy::Burst,
                50,
                Duration::from_secs(10),
                Duration::from_secs(10),
                1,
            ),
            (
                RateLimitStrategy::Fixed,
                50,
                Duration::from_secs(10),
                Duration::from_secs(10),
                50,
            ),
        ];

        for (strategy, capacity, window, expected_refill_every, expected_refill_amount) in
            test_cases
        {
            let rule =
                httpward_rate_limit_module::httpward_rate_limit_config::InternalRateLimitRule {
                    key: RateLimitKeyKind::Ip,
                    capacity,
                    refill_every: window,
                    refill_amount: 1,
                    strategy,
                };

            let runtime_rule = rule.to_runtime_rule();
            assert_eq!(runtime_rule.capacity, capacity);
            assert_eq!(runtime_rule.refill_every, expected_refill_every);
            assert_eq!(runtime_rule.refill_amount, expected_refill_amount);
        }
    }

    #[test]
    fn test_strategy_edge_cases() {
        // Test zero capacity edge case
        let rule = InternalRateLimitRule {
            key: RateLimitKeyKind::Ip,
            capacity: 0,
            refill_every: Duration::from_secs(10),
            refill_amount: 1,
            strategy: RateLimitStrategy::Sliding,
        };

        let runtime_rule = rule.to_runtime_rule();
        assert_eq!(runtime_rule.capacity, 1); // Should be sanitized to minimum 1
        assert!(!runtime_rule.refill_every.is_zero()); // Should not be zero
    }

    #[test]
    fn test_full_config_with_all_strategies() {
        let mut global_rules = HashMap::new();
        global_rules.insert(
            "ip".to_string(),
            RateLimitRuleConfig {
                max_requests: 50,
                window: "10s".to_string(),
                strategy: RateLimitStrategy::Sliding,
            },
        );

        let mut site_rules = HashMap::new();
        site_rules.insert(
            "ja4".to_string(),
            RateLimitRuleConfig {
                max_requests: 1000,
                window: "300s".to_string(),
                strategy: RateLimitStrategy::Burst,
            },
        );

        let config = HttpWardRateLimitConfig {
            global_config: None,
            global_rules: vec![global_rules],
            current_site_rules: vec![site_rules],
            response: None,
        };

        let internal = config.to_internal();

        // Check global sliding rule
        assert_eq!(internal.global.len(), 1);
        assert_eq!(internal.global[0].strategy, RateLimitStrategy::Sliding);
        assert_eq!(internal.global[0].capacity, 50);

        // Check site burst rule
        assert_eq!(internal.matched_route.len(), 1);
        assert_eq!(internal.matched_route[0].strategy, RateLimitStrategy::Burst);
        assert_eq!(internal.matched_route[0].capacity, 1000);
    }
}
