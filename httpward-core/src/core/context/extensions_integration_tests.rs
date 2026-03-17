// Integration tests for HttpWardContext extensions

#[cfg(test)]
mod integration_tests {
    use crate::core::HttpWardContext;
    use crate::core::context::ExtensionsMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Clone, Debug, PartialEq)]
    struct MockAnalysis {
        score: u32,
        label: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct MockClaims {
        user_id: u64,
        scopes: Vec<String>,
    }

    #[test]
    fn test_extensions_basic_operations() {
        let ext = ExtensionsMap::new();

        // Insert
        ext.insert("key1", 42u64);
        ext.insert("key2", "value".to_string());

        // Get
        assert_eq!(ext.get::<u64>("key1").map(|v| *v), Some(42u64));
        assert_eq!(ext.get::<String>("key2").map(|v| (*v).clone()), Some("value".to_string()));

        // Contains
        assert!(ext.contains_key("key1"));
        assert!(ext.contains_key("key2"));
        assert!(!ext.contains_key("key3"));

        // Length
        assert_eq!(ext.len(), 2);
    }

    #[test]
    fn test_extensions_type_safety() {
        let ext = ExtensionsMap::new();
        ext.insert("data", MockAnalysis {
            score: 85,
            label: "high_risk".to_string(),
        });

        // Correct type
        assert_eq!(
            ext.get::<MockAnalysis>("data"),
            Some(Arc::new(MockAnalysis {
                score: 85,
                label: "high_risk".to_string(),
            }))
        );

        // Wrong type
        assert_eq!(ext.get::<String>("data"), None);
        assert_eq!(ext.get::<u32>("data"), None);
    }

    #[test]
    fn test_extensions_remove() {
        let ext = ExtensionsMap::new();
        ext.insert("key", "value".to_string());

        assert!(ext.contains_key("key"));
        let removed = ext.remove("key");
        assert!(removed.is_some());
        assert!(!ext.contains_key("key"));
    }

    #[test]
    fn test_extensions_clear() {
        let ext = ExtensionsMap::new();
        ext.insert("key1", 1u64);
        ext.insert("key2", 2u64);
        ext.insert("key3", 3u64);

        assert_eq!(ext.len(), 3);
        ext.clear();
        assert_eq!(ext.len(), 0);
        assert!(ext.is_empty());
    }

    #[test]
    fn test_extensions_multiple_types() {
        let ext = ExtensionsMap::new();

        let analysis = MockAnalysis {
            score: 50,
            label: "medium".to_string(),
        };

        let claims = MockClaims {
            user_id: 123,
            scopes: vec!["read".to_string(), "write".to_string()],
        };

        ext.insert("analysis", analysis.clone());
        ext.insert("claims", claims.clone());

        assert_eq!(ext.get::<MockAnalysis>("analysis").map(|v| (*v).clone()), Some(analysis));
        assert_eq!(ext.get::<MockClaims>("claims").map(|v| (*v).clone()), Some(claims));
    }

    #[test]
    fn test_extensions_arc_sharing() {
        let ext = ExtensionsMap::new();
        ext.insert("shared", Arc::new(AtomicU64::new(0)));

        // Get the shared atomic
        if let Some(counter) = ext.get::<Arc<AtomicU64>>("shared") {
            (**counter).fetch_add(1, Ordering::Relaxed);
            assert_eq!((**counter).load(Ordering::Relaxed), 1);
        }

        // Get it again and verify it was modified
        if let Some(counter) = ext.get::<Arc<AtomicU64>>("shared") {
            assert_eq!((**counter).load(Ordering::Relaxed), 1);
            (**counter).fetch_add(1, Ordering::Relaxed);
        }

        // Verify final state
        if let Some(counter) = ext.get::<Arc<AtomicU64>>("shared") {
            assert_eq!((**counter).load(Ordering::Relaxed), 2);
        }
    }

    #[test]
    fn test_extensions_clone_independence() {
        let ext1 = ExtensionsMap::new();
        ext1.insert("key", "value1".to_string());

        let ext2 = ext1.clone();

        // Both see the same data
        assert_eq!(ext2.get::<String>("key").map(|v| (*v).clone()), Some("value1".to_string()));

        // Modifications are visible to both (same underlying storage)
        ext1.insert("key2", "value2".to_string());
        assert!(ext2.contains_key("key2"));
    }

    #[test]
    fn test_extensions_overwrite() {
        let ext = ExtensionsMap::new();

        ext.insert("key", 1u64);
        assert_eq!(ext.get::<u64>("key").map(|v| *v), Some(1u64));

        ext.insert("key", 2u64);
        assert_eq!(ext.get::<u64>("key").map(|v| *v), Some(2u64));
    }

    #[test]
    fn test_extensions_missing_keys() {
        let ext = ExtensionsMap::new();

        assert_eq!(ext.get::<u64>("nonexistent"), None);
        assert!(!ext.contains_key("nonexistent"));
        assert!(ext.remove("nonexistent").is_none());
    }

    #[test]
    fn test_complex_middleware_flow() {
        let ext = ExtensionsMap::new();

        // Simulate Middleware 1: Fingerprinting
        let analysis = MockAnalysis {
            score: 75,
            label: "medium_risk".to_string(),
        };
        ext.insert("user_analysis", analysis);

        // Simulate Middleware 2: JWT Validation
        let claims = MockClaims {
            user_id: 42,
            scopes: vec!["admin".to_string()],
        };
        ext.insert("jwt_claims", claims);

        // Simulate Middleware 3: Security Decision
        let final_decision = match (
            ext.get::<MockAnalysis>("user_analysis"),
            ext.get::<MockClaims>("jwt_claims"),
        ) {
            (Some(analysis), Some(claims)) => {
                if analysis.score > 50 && claims.scopes.contains(&"admin".to_string()) {
                    "HIGH_RISK_ADMIN"
                } else {
                    "NORMAL"
                }
            }
            _ => "NO_DATA",
        };

        assert_eq!(final_decision, "HIGH_RISK_ADMIN");
    }

    #[test]
    fn test_extensions_with_vec() {
        let ext = ExtensionsMap::new();

        let items = vec![1u64, 2, 3, 4, 5];
        ext.insert("numbers", items);

        if let Some(numbers) = ext.get::<Vec<u64>>("numbers") {
            assert_eq!((**numbers).len(), 5);
            assert_eq!((**numbers)[0], 1);
            assert_eq!((**numbers)[4], 5);
        } else {
            panic!("Failed to retrieve numbers");
        }
    }

    #[test]
    fn test_extensions_empty_check() {
        let ext = ExtensionsMap::new();
        assert!(ext.is_empty());
        assert_eq!(ext.len(), 0);

        ext.insert("key", 1u64);
        assert!(!ext.is_empty());
        assert_eq!(ext.len(), 1);
    }
}





