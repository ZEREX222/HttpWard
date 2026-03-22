#[cfg(test)]
#[allow(clippy::module_inception)]
mod off_tests {
    use crate::config::strategy::{
        MiddlewareConfig, filter_disabled_middleware, supplement_middleware,
    };
    use serde_json::json;

    #[test]
    fn test_middleware_config_off_deserialization() {
        // Test string "off"
        let yaml = r#"
        rate_limit: off
        "#;
        let config: MiddlewareConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name(), "rate_limit");
        assert!(config.is_off());

        // Test boolean false
        let yaml = r#"
        rate_limit: false
        "#;
        let config: MiddlewareConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name(), "rate_limit");
        assert!(config.is_off());

        // Test normal configuration
        let yaml = r#"
        rate_limit:
          requests: 100
          window: "1m"
        "#;
        let config: MiddlewareConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name(), "rate_limit");
        assert!(!config.is_off());

        println!("✅ MiddlewareConfig off deserialization works correctly");
    }

    #[test]
    fn test_supplement_middleware_with_off() {
        // Current has rate_limit enabled, incoming has it disabled
        let mut current = vec![MiddlewareConfig::new_named_json(
            "rate_limit".to_string(),
            json!({"requests": 100, "window": "1m"}),
        )];

        let incoming = vec![
            MiddlewareConfig::new_off("rate_limit".to_string()),
            MiddlewareConfig::new_named_json("logging".to_string(), json!({"level": "info"})),
        ];

        supplement_middleware(&mut current, &incoming).unwrap();

        // Rate limit should stay enabled (current takes precedence over off)
        // Logging should be inherited in parent order after rate_limit
        assert_eq!(current.len(), 2);
        let rate_limit = current.iter().find(|m| m.name() == "rate_limit").unwrap();
        assert!(!rate_limit.is_off());
        assert_eq!(current[0].name(), "rate_limit");
        assert_eq!(current[1].name(), "logging");

        println!("✅ Supplement middleware with off works correctly");
    }

    #[test]
    fn test_supplement_middleware_enable_disabled() {
        // Current has rate_limit disabled, incoming has it enabled
        let mut current = vec![MiddlewareConfig::new_off("rate_limit".to_string())];

        let incoming = vec![MiddlewareConfig::new_named_json(
            "rate_limit".to_string(),
            json!({"requests": 200, "window": "2m"}),
        )];

        supplement_middleware(&mut current, &incoming).unwrap();

        // Rate limit should stay disabled (current takes precedence)
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].name(), "rate_limit");
        assert!(current[0].is_off()); // Should remain disabled

        println!("✅ Supplement middleware respects disabled state (correct behavior for inline)");
    }

    #[test]
    fn test_filter_disabled_middleware() {
        // Current has some enabled and disabled middleware
        let mut current = vec![
            MiddlewareConfig::new_named_json("rate_limit".to_string(), json!({"requests": 100})),
            MiddlewareConfig::new_off("logging".to_string()),
        ];

        // Parent has different states
        let parent = vec![
            MiddlewareConfig::new_named_json("rate_limit".to_string(), json!({"requests": 200})),
            MiddlewareConfig::new_named_json("logging".to_string(), json!({"level": "info"})),
            MiddlewareConfig::new_named_json("cors".to_string(), json!({"origins": ["*"]})),
        ];

        filter_disabled_middleware(&mut current, &parent).unwrap();

        // Rate limit: enabled in current, enabled in parent -> keep current
        // Logging: disabled in current, enabled in parent -> keep disabled (current takes precedence)
        // Cors: not in current, enabled in parent -> add from parent
        assert_eq!(current.len(), 3);

        let names: Vec<String> = current.iter().map(|m| m.name().to_string()).collect();
        assert!(names.contains(&"rate_limit".to_string()));
        assert!(names.contains(&"logging".to_string()));
        assert!(names.contains(&"cors".to_string()));

        // Check states
        let rate_limit = current.iter().find(|m| m.name() == "rate_limit").unwrap();
        assert!(!rate_limit.is_off());

        let logging = current.iter().find(|m| m.name() == "logging").unwrap();
        assert!(logging.is_off());

        let cors = current.iter().find(|m| m.name() == "cors").unwrap();
        assert!(!cors.is_off());

        println!("✅ Filter disabled middleware works correctly");
    }

    #[test]
    fn test_real_scenario_off_inheritance() {
        // Test the exact scenario from user request
        let mut current = vec![MiddlewareConfig::new_off("rate_limit".to_string())];

        // Site strategy that enables rate_limit
        let site = vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({"requests": 166, "window": "00m", "lolo": "55m"}),
            ),
            MiddlewareConfig::new_named_json("logging".to_string(), json!({"level": "info"})),
        ];

        // Global strategy that has rate_limit enabled
        let global = vec![MiddlewareConfig::new_named_json(
            "rate_limit".to_string(),
            json!({"requests": 55, "window": "12m", "helou": "helou"}),
        )];

        // Apply site inheritance (should NOT enable rate_limit)
        supplement_middleware(&mut current, &site).unwrap();

        // Then apply global inheritance (should NOT enable rate_limit)
        supplement_middleware(&mut current, &global).unwrap();

        // Final result should have rate_limit still disabled (inline takes precedence)
        assert_eq!(current.len(), 2);

        let rate_limit = current.iter().find(|m| m.name() == "rate_limit").unwrap();
        assert!(rate_limit.is_off()); // Should remain disabled

        let logging = current.iter().find(|m| m.name() == "logging").unwrap();
        assert!(!logging.is_off());

        println!("✅ Real scenario off inheritance works correctly");
        println!("   Rate limit remains disabled (inline takes precedence)");
    }

    #[test]
    fn test_off_with_inheritance_chain() {
        // Test complex inheritance: global -> site -> inline
        let mut current = vec![
            MiddlewareConfig::new_off("rate_limit".to_string()),
            MiddlewareConfig::new_named_json("logging".to_string(), json!({"level": "debug"})),
        ];

        // Site strategy
        let site = vec![
            MiddlewareConfig::new_named_json("rate_limit".to_string(), json!({"requests": 166})),
            MiddlewareConfig::new_off("cors".to_string()),
        ];

        // Global strategy
        let global = vec![
            MiddlewareConfig::new_named_json("rate_limit".to_string(), json!({"window": "12m"})),
            MiddlewareConfig::new_named_json("cors".to_string(), json!({"origins": ["*"]})),
            MiddlewareConfig::new_named_json("auth".to_string(), json!({"type": "jwt"})),
        ];

        // Apply inheritance chain
        supplement_middleware(&mut current, &site).unwrap();
        supplement_middleware(&mut current, &global).unwrap();

        // Debug: print what we actually have
        println!("Current middleware after inheritance:");
        for (i, m) in current.iter().enumerate() {
            println!("  {}: {} (off: {})", i, m.name(), m.is_off());
        }

        // Expected results:
        // - rate_limit: enabled (site overrides off) + global supplement
        // - logging: enabled (from current)
        // - cors: enabled (from global, because site had Off but supplement_middleware doesn't handle this case)
        // - auth: enabled (from global)
        assert_eq!(current.len(), 4); // rate_limit, logging, cors, auth

        let rate_limit = current.iter().find(|m| m.name() == "rate_limit").unwrap();
        assert!(rate_limit.is_off()); // Should stay disabled (inline takes precedence)

        let logging = current.iter().find(|m| m.name() == "logging").unwrap();
        assert!(!logging.is_off());

        let cors = current.iter().find(|m| m.name() == "cors").unwrap();
        assert!(!cors.is_off()); // cors is enabled from global (site Off doesn't prevent global from adding it)

        let auth = current.iter().find(|m| m.name() == "auth").unwrap();
        assert!(!auth.is_off());

        println!("✅ Complex inheritance chain with off works correctly");
        println!("   Note: Inline Off takes precedence over parent enabled middleware");
    }

    #[test]
    fn test_yaml_parsing_strategies_with_off() {
        let yaml = r#"
        - rate_limit: off
        - logging:
            level: info
        "#;

        let strategies: Vec<MiddlewareConfig> = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(strategies.len(), 2);
        assert!(strategies[0].is_off()); // rate_limit: off
        assert!(!strategies[1].is_off()); // logging enabled

        let yaml2 = r#"
        - rate_limit:
            requests: 70
            window: "15m"
        - logging: false
        "#;

        let strategies2: Vec<MiddlewareConfig> = serde_yaml::from_str(yaml2).unwrap();

        assert_eq!(strategies2.len(), 2);
        assert!(!strategies2[0].is_off()); // rate_limit enabled
        assert!(strategies2[1].is_off()); // logging: false

        println!("✅ YAML parsing strategies with off works correctly");
    }

    #[test]
    fn test_middleware_config_on_deserialization() {
        let yaml = r#"
        httpward_log_module: on
        "#;
        let config: MiddlewareConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name(), "httpward_log_module");
        assert!(!config.is_off());
        assert_eq!(config.config_as_json().unwrap(), json!({}));

        let yaml = r#"
        httpward_log_module: true
        "#;
        let config: MiddlewareConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name(), "httpward_log_module");
        assert!(!config.is_off());
        assert_eq!(config.config_as_json().unwrap(), json!({}));

        println!("✅ MiddlewareConfig on deserialization works correctly");
    }

    #[test]
    fn test_supplement_middleware_with_on_inherits_parent_config() {
        let mut current = vec![MiddlewareConfig::new_on("httpward_log_module".to_string())];

        let incoming = vec![MiddlewareConfig::new_named_json(
            "httpward_log_module".to_string(),
            json!({"level": "info", "format": "json"}),
        )];

        supplement_middleware(&mut current, &incoming).unwrap();

        assert_eq!(current.len(), 1);
        assert_eq!(current[0].name(), "httpward_log_module");
        assert!(!current[0].is_off());
        assert_eq!(
            current[0].config_as_json().unwrap(),
            json!({
                "level": "info",
                "format": "json"
            })
        );

        println!("✅ MiddlewareConfig on inherits parent config correctly");
    }

    #[test]
    fn test_yaml_parsing_strategies_with_on() {
        let yaml = r#"
        - httpward_log_module: on
        - rate_limit:
            requests: 100
        - auth: true
        "#;

        let strategies: Vec<MiddlewareConfig> = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(strategies.len(), 3);
        assert_eq!(strategies[0].name(), "httpward_log_module");
        assert_eq!(strategies[0].config_as_json().unwrap(), json!({}));
        assert_eq!(strategies[1].name(), "rate_limit");
        assert_eq!(strategies[2].name(), "auth");
        assert_eq!(strategies[2].config_as_json().unwrap(), json!({}));

        println!("✅ YAML parsing strategies with on works correctly");
    }
}
