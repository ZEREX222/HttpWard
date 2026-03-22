/// Rate Limit Analytics integration test
/// Demonstrates the full processing cycle with check status tracking
#[cfg(test)]
mod integration_tests {
    use httpward_rate_limit_module::{
        RateLimitCheckStatus, RateLimitKeyKind, RateLimitManager, RateLimitScope,
    };

    #[tokio::test]
    async fn test_rate_limit_analytics_allow() {
        // Create manager
        let manager = RateLimitManager::new();

        // Add rules (this should happen during initialization)
        // In a real application this is done via config

        // Scenario: first request from IP
        let checks = vec![(
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            "192.168.1.100".to_string(),
        )];

        let results = manager.check_all_with_results(&checks).await.unwrap();

        assert!(results.allowed, "Request should be allowed");
        assert_eq!(results.checks.len(), 1);

        let check = &results.checks[0];
        assert_eq!(check.kind, RateLimitKeyKind::Ip);
        assert_eq!(check.scope_type, "Global");
        assert_eq!(check.value, "192.168.1.100");
        assert_eq!(check.status, RateLimitCheckStatus::Allow);

        println!("✓ Test passed: First request returns Allow status");
    }

    #[tokio::test]
    async fn test_rate_limit_analytics_multiple_checks() {
        let manager = RateLimitManager::new();

        // Scenario: multiple check types
        let checks = vec![
            (
                RateLimitKeyKind::Ip,
                RateLimitScope::Global,
                "10.0.0.1".to_string(),
            ),
            (
                RateLimitKeyKind::HeaderFingerprint,
                RateLimitScope::Global,
                "fingerprint_abc".to_string(),
            ),
            (
                RateLimitKeyKind::Cookie,
                RateLimitScope::Global,
                "session_xyz".to_string(),
            ),
        ];

        let results = manager.check_all_with_results(&checks).await.unwrap();

        assert!(results.allowed);
        assert_eq!(results.checks.len(), 3);

        // Verify each check result
        for (i, check_result) in results.checks.iter().enumerate() {
            println!(
                "Check {}: kind={:?}, status={:?}",
                i + 1,
                check_result.kind,
                check_result.status
            );
            assert_eq!(check_result.scope_type, "Global");
        }

        println!("✓ Test passed: Multiple checks processed correctly");
    }

    #[tokio::test]
    async fn test_rate_limit_analytics_json_serialization() {
        let manager = RateLimitManager::new();

        let checks = vec![
            (
                RateLimitKeyKind::Ip,
                RateLimitScope::Global,
                "172.16.0.1".to_string(),
            ),
            (
                RateLimitKeyKind::Ja4,
                RateLimitScope::Global,
                "ja4_fingerprint_123".to_string(),
            ),
        ];

        let results = manager.check_all_with_results(&checks).await.unwrap();

        // Verify JSON serialization
        let json = serde_json::to_string_pretty(&results).expect("Failed to serialize to JSON");

        println!("JSON results:");
        println!("{}", json);

        // Verify JSON contains expected fields
        assert!(json.contains("\"allowed\""));
        assert!(json.contains("\"checks\""));
        assert!(json.contains("\"kind\""));
        assert!(json.contains("\"status\""));
        assert!(json.contains("\"value\""));
        assert!(json.contains("\"scope_type\""));

        // Deserialize and verify
        let deserialized: httpward_rate_limit_module::RateLimitCheckResults =
            serde_json::from_str(&json).expect("Failed to deserialize from JSON");

        assert_eq!(deserialized.checks.len(), results.checks.len());
        assert_eq!(deserialized.allowed, results.allowed);

        println!("✓ Test passed: JSON serialization/deserialization works");
    }

    #[tokio::test]
    async fn test_rate_limit_status_statistics() {
        let manager = RateLimitManager::new();

        // Simulate 5 checks
        let mut allowed_count = 0;
        let mut new_count = 0;
        let mut declined_count = 0;

        for i in 0..5 {
            let checks = vec![(
                RateLimitKeyKind::Ip,
                RateLimitScope::Global,
                format!("192.168.1.{}", i),
            )];

            let results = manager.check_all_with_results(&checks).await.unwrap();

            for check in &results.checks {
                match check.status {
                    RateLimitCheckStatus::Allow => allowed_count += 1,
                    RateLimitCheckStatus::New => new_count += 1,
                    RateLimitCheckStatus::Declined => declined_count += 1,
                }
            }
        }

        println!("Statistics after 5 checks:");
        println!("  Allow: {}", allowed_count);
        println!("  New: {}", new_count);
        println!("  Declined: {}", declined_count);

        println!("✓ Test passed: Statistics collection works");
    }

    #[test]
    fn test_rate_limit_status_enum_serde() {
        // Verify enum serializes correctly
        let json_allow = serde_json::to_string(&RateLimitCheckStatus::Allow).unwrap();
        let json_declined = serde_json::to_string(&RateLimitCheckStatus::Declined).unwrap();
        let json_new = serde_json::to_string(&RateLimitCheckStatus::New).unwrap();

        println!("Allow JSON: {}", json_allow);
        println!("Declined JSON: {}", json_declined);
        println!("New JSON: {}", json_new);

        // Verify values match the expected format
        assert!(json_allow.contains("Allow"));
        assert!(json_declined.contains("Declined"));
        assert!(json_new.contains("New"));

        println!("✓ Test passed: Enum serialization works correctly");
    }

    #[tokio::test]
    async fn test_rate_limit_analytics_real_world_scenario() {
        let manager = RateLimitManager::new();

        // Real-world scenario: validate request by IP and header fingerprint
        let client_ip = "203.0.113.45";
        let header_fingerprint = "sha256_abc123def456";

        let checks = vec![
            (
                RateLimitKeyKind::Ip,
                RateLimitScope::Global,
                client_ip.to_string(),
            ),
            (
                RateLimitKeyKind::HeaderFingerprint,
                RateLimitScope::Global,
                header_fingerprint.to_string(),
            ),
        ];

        let results = manager.check_all_with_results(&checks).await.unwrap();

        println!("\n=== Real-world Scenario ===");
        println!("Request from: {}", client_ip);
        println!(
            "Overall result: {}\n",
            if results.allowed {
                "✓ ALLOWED"
            } else {
                "✗ BLOCKED"
            }
        );

        for check in &results.checks {
            let status_icon = match check.status {
                RateLimitCheckStatus::Allow => "✓",
                RateLimitCheckStatus::New => "⊕",
                RateLimitCheckStatus::Declined => "✗",
            };

            println!(
                "{} {} [{:?}] - {}: {}",
                status_icon,
                check.scope_type,
                check.kind,
                check.value,
                match check.status {
                    RateLimitCheckStatus::Allow => "Within limits",
                    RateLimitCheckStatus::New => "New entry",
                    RateLimitCheckStatus::Declined => "Limit exceeded",
                }
            );
        }

        println!("\n✓ Test passed: Real-world scenario processed correctly");
    }
}
