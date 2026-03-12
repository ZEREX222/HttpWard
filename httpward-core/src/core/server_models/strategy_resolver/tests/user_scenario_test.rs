#[cfg(test)]
mod user_scenario_test {
    use crate::config::global::{GlobalConfig, Route, Match};
    use crate::config::strategy::{StrategyRef, MiddlewareConfig};
    use crate::config::SiteConfig;
    use crate::core::server_models::strategy_resolver::StrategyResolver;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn test_user_exact_scenario() {
        // Exact scenario from user:
        // - Global has default2 with rate_limit enabled
        // - Site has default55 with rate_limit enabled  
        // - Route has inline strategy with rate_limit: off
        
        let global = GlobalConfig {
            domain: "global.local".to_string(),
            domains: vec![],
            listeners: vec![],
            routes: vec![],
            log: crate::config::global::LogConfig::default(),
            sites_enabled: PathBuf::from("./sites-enabled"),
            strategy: Some(StrategyRef::Named("default2".to_string())),
            strategies: {
                let mut strategies = std::collections::HashMap::new();
                strategies.insert("default2".to_string(), vec![
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({"requests": 55, "window": "12m", "helou": "helou"})
                    ),
                    MiddlewareConfig::new_named_json(
                        "logging".to_string(),
                        json!({"level": "info"})
                    ),
                ]);
                strategies
            },
        };

        let site = SiteConfig {
            domain: "test.local".to_string(),
            domains: vec!["test.local".to_string(), "*.test2.local".to_string()],
            listeners: vec![],
            routes: vec![
                Route::Static {
                    r#match: Match {
                        path: Some("/site/{*path}".to_string()),
                        path_regex: None,
                    },
                    static_dir: PathBuf::from("/myprojects/html"),
                    strategy: Some(StrategyRef::InlineMiddleware(vec![
                        MiddlewareConfig::new_off("rate_limit".to_string())  // User's exact scenario
                    ])),
                    strategies: None,
                }
            ],
            strategy: Some(StrategyRef::Named("default55".to_string())),
            strategies: {
                let mut strategies = std::collections::HashMap::new();
                strategies.insert("default55".to_string(), vec![
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({"requests": 166, "window": "00m", "lolo": "55m", "additional": {"site1": "site1", "site2": "site2"}})
                    ),
                    MiddlewareConfig::new_named_json(
                        "logging".to_string(),
                        json!({"level": "info"})
                    ),
                ]);
                strategies
            },
        };
        
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let inline_strategy = resolver.resolve_for_route(0).unwrap();
        
        // Debug: print what we actually have
        println!("User scenario - final middleware:");
        for (i, m) in inline_strategy.middleware.iter().enumerate() {
            println!("  {}: {} (off: {})", i, m.name(), m.is_off());
        }
        
        // Should NOT contain rate_limit because it's disabled in inline
        let rate_limit_middleware = inline_strategy.middleware.iter()
            .find(|m| m.name() == "rate_limit");
        
        assert!(rate_limit_middleware.is_none(), 
               "rate_limit should be completely removed when disabled in inline strategy");
        
        // Should contain logging because it's not disabled and gets inherited
        let logging_middleware = inline_strategy.middleware.iter()
            .find(|m| m.name() == "logging");
        assert!(logging_middleware.is_some(), "logging should be present from inheritance");
        
        println!("✅ User exact scenario works correctly - rate_limit is completely removed");
        println!("   This matches the expected behavior: inline 'rate_limit: off' removes it completely");
    }
}
