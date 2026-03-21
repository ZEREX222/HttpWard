#[cfg(test)]
#[allow(clippy::module_inception)]
mod off_inheritance_tests {
    use crate::config::SiteConfig;
    use crate::config::global::{GlobalConfig, Match, Route};
    use crate::config::strategy::{MiddlewareConfig, StrategyRef};
    use crate::core::server_models::strategy_resolver::StrategyResolver;
    use serde_json::json;
    use std::path::PathBuf;

    fn create_global_config() -> GlobalConfig {
        use crate::config::global::LogConfig;

        GlobalConfig {
            domain: "global.local".to_string(),
            domains: vec![],
            listeners: vec![],
            routes: vec![],
            log: LogConfig::default(),
            proxy_id: "httpward".to_string(),
            sites_enabled: PathBuf::from("./sites-enabled"),
            strategy: Some(StrategyRef::Named("default2".to_string())),
            strategies: {
                let mut strategies = std::collections::HashMap::new();
                strategies.insert(
                    "default2".to_string(),
                    vec![
                        MiddlewareConfig::new_named_json(
                            "rate_limit".to_string(),
                            json!({"requests": 55, "window": "12m", "helou": "helou"}),
                        ),
                        MiddlewareConfig::new_named_json(
                            "logging".to_string(),
                            json!({"level": "info"}),
                        ),
                    ],
                );
                strategies
            },
        }
    }

    fn create_site_config_with_off_route() -> SiteConfig {
        SiteConfig {
            domain: "test.local".to_string(),
            domains: vec!["test.local".to_string(), "*.test2.local".to_string()],
            listeners: vec![],
            routes: vec![Route::Static {
                r#match: Match {
                    path: Some("/site/{*path}".to_string()),
                    path_regex: None,
                },
                static_dir: PathBuf::from("/myprojects/html"),
                strategy: Some(StrategyRef::InlineMiddleware(vec![
                    MiddlewareConfig::new_off("rate_limit".to_string()),
                ])),
                strategies: None,
            }],
            strategy: Some(StrategyRef::Named("default55".to_string())),
            strategies: {
                let mut strategies = std::collections::HashMap::new();
                strategies.insert(
                    "default55".to_string(),
                    vec![
                        MiddlewareConfig::new_named_json(
                            "rate_limit".to_string(),
                            json!({"requests": 166, "window": "00m", "lolo": "55m"}),
                        ),
                        MiddlewareConfig::new_named_json(
                            "logging".to_string(),
                            json!({"level": "info"}),
                        ),
                    ],
                );
                strategies
            },
        }
    }

    #[test]
    fn test_inline_off_removes_inherited_middleware() {
        let global = create_global_config();
        let site = create_site_config_with_off_route();
        let resolver = StrategyResolver::new(&site, &global).unwrap();

        // Get the inline strategy from route
        let inline_strategy = resolver.resolve_for_route(0).unwrap();

        // Debug: print what we actually have
        println!("Debug - inline strategy middleware:");
        for (i, m) in inline_strategy.middleware.iter().enumerate() {
            println!("  {}: {} (off: {})", i, m.name(), m.is_off());
        }

        // Should NOT contain rate_limit because it's disabled in inline
        let middleware_names: Vec<String> = inline_strategy
            .middleware
            .iter()
            .map(|m| m.name().to_string())
            .collect();

        println!("Debug - middleware names: {:?}", middleware_names);

        assert!(
            !middleware_names.contains(&"rate_limit".to_string()),
            "rate_limit should be removed because it's disabled in inline"
        );

        // Should contain logging because it's not disabled and gets inherited
        assert!(
            middleware_names.contains(&"logging".to_string()),
            "logging should be present because it's inherited from site/global"
        );

        println!("✅ Inline off correctly removes inherited middleware");
        println!("   Final middleware: {:?}", middleware_names);
    }

    #[test]
    fn test_inline_off_with_complex_inheritance() {
        let global = create_global_config();

        // Create site with rate_limit enabled
        let mut site = create_site_config_with_off_route();
        site.strategies.get_mut("default55").unwrap()[0] = MiddlewareConfig::new_named_json(
            "rate_limit".to_string(),
            json!({"requests": 166, "window": "00m", "lolo": "55m", "additional": {"site1": "site1"}}),
        );

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let inline_strategy = resolver.resolve_for_route(0).unwrap();

        // Rate_limit should be completely removed (not even with inherited config)
        let rate_limit_middleware = inline_strategy
            .middleware
            .iter()
            .find(|m| m.name() == "rate_limit");

        assert!(
            rate_limit_middleware.is_none(),
            "rate_limit should be completely removed when disabled in inline"
        );

        // Logging should be present
        let logging_middleware = inline_strategy
            .middleware
            .iter()
            .find(|m| m.name() == "logging");
        assert!(logging_middleware.is_some(), "logging should be present");

        println!("✅ Inline off completely removes middleware with complex inheritance");
    }

    #[test]
    fn test_inline_partial_off() {
        let global = create_global_config();

        // Create site with multiple middleware
        let mut site = create_site_config_with_off_route();
        site.routes[0] = Route::Static {
            r#match: Match {
                path: Some("/site/{*path}".to_string()),
                path_regex: None,
            },
            static_dir: PathBuf::from("/myprojects/html"),
            strategy: Some(StrategyRef::InlineMiddleware(vec![
                MiddlewareConfig::new_off("rate_limit".to_string()), // Disabled
                MiddlewareConfig::new_named_json(
                    // Enabled
                    "cors".to_string(),
                    json!({"origins": ["*"]}),
                ),
            ])),
            strategies: None,
        };

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let inline_strategy = resolver.resolve_for_route(0).unwrap();

        let middleware_names: Vec<String> = inline_strategy
            .middleware
            .iter()
            .map(|m| m.name().to_string())
            .collect();

        // Should contain cors (from inline) and logging (from inheritance)
        assert!(
            middleware_names.contains(&"cors".to_string()),
            "cors should be present"
        );
        assert!(
            middleware_names.contains(&"logging".to_string()),
            "logging should be present"
        );

        // Should NOT contain rate_limit (disabled in inline)
        assert!(
            !middleware_names.contains(&"rate_limit".to_string()),
            "rate_limit should be removed"
        );

        println!("✅ Partial off works correctly - some middleware kept, others removed");
        println!("   Final middleware: {:?}", middleware_names);
    }
}
