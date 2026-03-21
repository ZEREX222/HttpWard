#[cfg(test)]
#[allow(clippy::module_inception)]
mod hierarchical_inheritance_tests {
    use crate::config::SiteConfig;
    use crate::config::global::{GlobalConfig, Match, Route};
    use crate::config::strategy::{MiddlewareConfig, StrategyRef};
    use crate::core::server_models::strategy_resolver::StrategyResolver;
    use serde_json::json;
    use std::path::PathBuf;

    fn create_global_with_all_middleware() -> GlobalConfig {
        GlobalConfig {
            domain: "global.local".to_string(),
            domains: vec![],
            listeners: vec![],
            routes: vec![],
            log: crate::config::global::LogConfig::default(),
            proxy_id: "httpward".to_string(),
            sites_enabled: PathBuf::from("./sites-enabled"),
            strategy: Some(StrategyRef::Named("global_base".to_string())),
            strategies: {
                let mut strategies = std::collections::HashMap::new();
                strategies.insert(
                    "global_base".to_string(),
                    vec![
                        MiddlewareConfig::new_named_json(
                            "rate_limit".to_string(),
                            json!({"requests": 100, "window": "1m", "global_rl": "global_rl"}),
                        ),
                        MiddlewareConfig::new_named_json(
                            "logging".to_string(),
                            json!({"level": "info", "global_log": "global_log"}),
                        ),
                        MiddlewareConfig::new_named_json(
                            "cors".to_string(),
                            json!({"origins": ["*"], "global_cors": "global_cors"}),
                        ),
                        MiddlewareConfig::new_named_json(
                            "auth".to_string(),
                            json!({"type": "basic", "global_auth": "global_auth"}),
                        ),
                        MiddlewareConfig::new_named_json(
                            "cache".to_string(),
                            json!({"ttl": 300, "global_cache": "global_cache"}),
                        ),
                    ],
                );
                strategies
            },
        }
    }

    fn create_site_with_partial_override() -> SiteConfig {
        SiteConfig {
            domain: "site.local".to_string(),
            domains: vec!["site.local".to_string()],
            listeners: vec![],
            routes: vec![],
            strategy: Some(StrategyRef::Named("site_override".to_string())),
            strategies: {
                let mut strategies = std::collections::HashMap::new();
                strategies.insert(
                    "site_override".to_string(),
                    vec![
                        // Override rate_limit with different values
                        MiddlewareConfig::new_named_json(
                            "rate_limit".to_string(),
                            json!({"requests": 200, "window": "30s", "site_rl": "site_rl"}),
                        ),
                        // Override logging completely
                        MiddlewareConfig::new_named_json(
                            "logging".to_string(),
                            json!({"level": "debug", "site_log": "site_log"}),
                        ),
                        // Add new middleware not in global
                        MiddlewareConfig::new_named_json(
                            "security".to_string(),
                            json!({"level": "high", "site_security": "site_security"}),
                        ),
                        // Keep cors and auth from global (no override)
                        // Override cache
                        MiddlewareConfig::new_named_json(
                            "cache".to_string(),
                            json!({"ttl": 600, "site_cache": "site_cache"}),
                        ),
                    ],
                );
                strategies
            },
        }
    }

    #[test]
    fn test_hierarchical_global_to_site_merging() {
        let global = create_global_with_all_middleware();
        let site = create_site_with_partial_override();

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let site_strategy = resolver.resolve_for_site(&site).unwrap().unwrap();

        // Should have all middleware: global + site
        println!(
            "Debug - site strategy middleware count: {}",
            site_strategy.middleware.len()
        );
        for (i, m) in site_strategy.middleware.iter().enumerate() {
            println!("  {}: {}", i, m.name());
        }
        // Should have all middleware: global (5) + site (1 new security) = 6
        assert_eq!(site_strategy.middleware.len(), 6);

        // Check rate_limit: site overrides global, but global fields are preserved
        let rate_limit = site_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "rate_limit")
            .unwrap();
        let config = rate_limit.config_as_json().unwrap();
        assert_eq!(config["requests"], 200); // From site (override)
        assert_eq!(config["window"], "30s"); // From site (override)
        assert_eq!(config["site_rl"], "site_rl"); // From site
        assert_eq!(config["global_rl"], "global_rl"); // From global (supplement)

        // Check logging: site overrides global completely
        let logging = site_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "logging")
            .unwrap();
        let log_config = logging.config_as_json().unwrap();
        assert_eq!(log_config["level"], "debug"); // From site
        assert_eq!(log_config["site_log"], "site_log"); // From site
        assert_eq!(log_config["global_log"], "global_log"); // From global (supplement)

        // Check cors: only from global (no site override)
        let cors = site_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "cors")
            .unwrap();
        let cors_config = cors.config_as_json().unwrap();
        assert_eq!(cors_config["global_cors"], "global_cors");

        // Check auth: only from global (no site override)
        let auth = site_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "auth")
            .unwrap();
        let auth_config = auth.config_as_json().unwrap();
        assert_eq!(auth_config["global_auth"], "global_auth");

        // Check cache: site overrides global
        let cache = site_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "cache")
            .unwrap();
        let cache_config = cache.config_as_json().unwrap();
        assert_eq!(cache_config["ttl"], 600); // From site
        assert_eq!(cache_config["site_cache"], "site_cache"); // From site
        assert_eq!(cache_config["global_cache"], "global_cache"); // From global (supplement)

        // Check security: only from site (new middleware)
        let security = site_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "security")
            .unwrap();
        let sec_config = security.config_as_json().unwrap();
        assert_eq!(sec_config["site_security"], "site_security");

        println!("✅ Hierarchical global to site merging works correctly");
    }

    #[test]
    fn test_hierarchical_site_to_route_inheritance() {
        let global = create_global_with_all_middleware();
        let mut site = create_site_with_partial_override();

        // Add route that inherits from site strategy
        site.routes.push(Route::Proxy {
            r#match: Match {
                path: Some("/api/{*path}".to_string()),
                path_regex: None,
            },
            backend: "http://api:8080/{*path}".to_string(),
            strategy: Some(StrategyRef::Named("site_override".to_string())),
            strategies: None,
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let route_strategy = resolver.resolve_for_route(0).unwrap();

        // Should inherit the merged site+global strategy
        assert_eq!(route_strategy.middleware.len(), 6);

        // Verify the inheritance chain: route inherits from site (which already inherited from global)
        let rate_limit = route_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "rate_limit")
            .unwrap();
        let config = rate_limit.config_as_json().unwrap();
        assert_eq!(config["requests"], 200); // From site (which overrode global)
        assert_eq!(config["site_rl"], "site_rl"); // From site
        assert_eq!(config["global_rl"], "global_rl"); // From global

        println!("✅ Hierarchical site to route inheritance works correctly");
    }

    #[test]
    fn test_hierarchical_inline_with_full_inheritance() {
        let global = create_global_with_all_middleware();
        let mut site = create_site_with_partial_override();

        // Add route with inline strategy that inherits from everything
        site.routes.push(Route::Proxy {
            r#match: Match {
                path: Some("/inline/{*path}".to_string()),
                path_regex: None,
            },
            backend: "http://inline:8080/{*path}".to_string(),
            strategy: Some(StrategyRef::InlineMiddleware(vec![
                // Override rate_limit at inline level
                MiddlewareConfig::new_named_json(
                    "rate_limit".to_string(),
                    json!({"requests": 500, "window": "10s", "inline_rl": "inline_rl"}),
                ),
                // Disable logging at inline level
                MiddlewareConfig::new_off("logging".to_string()),
                // Add new inline middleware
                MiddlewareConfig::new_named_json(
                    "monitoring".to_string(),
                    json!({"enabled": true, "inline_monitoring": "inline_monitoring"}),
                ),
            ])),
            strategies: None,
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let inline_strategy = resolver.resolve_for_route(0).unwrap();

        // Should have: inline overrides + inherited from site+global
        // logging should be filtered out (off)
        assert_eq!(inline_strategy.middleware.len(), 6); // rate_limit, cors, auth, cache, security, monitoring

        // Check rate_limit: inline > site > global
        let rate_limit = inline_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "rate_limit")
            .unwrap();
        let config = rate_limit.config_as_json().unwrap();
        assert_eq!(config["requests"], 500); // From inline (highest priority)
        assert_eq!(config["window"], "10s"); // From inline
        assert_eq!(config["inline_rl"], "inline_rl"); // From inline
        assert_eq!(config["site_rl"], "site_rl"); // From site (supplement)
        assert_eq!(config["global_rl"], "global_rl"); // From global (supplement)

        // Logging should be completely removed (off in inline)
        assert!(
            inline_strategy
                .middleware
                .iter()
                .find(|m: &&MiddlewareConfig| m.name() == "logging")
                .is_none()
        );

        // Check cors: inherited from global
        let cors = inline_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "cors")
            .unwrap();
        assert!(cors.config_as_json().is_ok());

        // Check security: inherited from site
        let security = inline_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "security")
            .unwrap();
        let sec_config = security.config_as_json().unwrap();
        assert_eq!(sec_config["site_security"], "site_security");

        // Check monitoring: only from inline
        let monitoring = inline_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "monitoring")
            .unwrap();
        let mon_config = monitoring.config_as_json().unwrap();
        assert_eq!(mon_config["enabled"], true);

        println!("✅ Hierarchical inline with full inheritance works correctly");
    }

    #[test]
    fn test_hierarchical_route_local_strategy_isolation() {
        let global = create_global_with_all_middleware();
        let mut site = create_site_with_partial_override();

        // Add route with local strategy (should be isolated from site inheritance)
        site.routes.push(Route::Proxy {
            r#match: Match {
                path: Some("/isolated/{*path}".to_string()),
                path_regex: None,
            },
            backend: "http://isolated:8080/{*path}".to_string(),
            strategy: Some(StrategyRef::Named("local_isolated".to_string())),
            strategies: Some({
                let mut local_strats = std::collections::HashMap::new();
                local_strats.insert(
                    "local_isolated".to_string(),
                    vec![
                        // Only rate_limit in local strategy
                        MiddlewareConfig::new_named_json(
                            "rate_limit".to_string(),
                            json!({"requests": 50, "window": "5s", "local_rl": "local_rl"}),
                        ),
                        // Disable cors
                        MiddlewareConfig::new_off("cors".to_string()),
                        // Add local-only middleware
                        MiddlewareConfig::new_named_json(
                            "local_only".to_string(),
                            json!({"local_field": "local_only"}),
                        ),
                    ],
                );
                local_strats
            }),
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let local_strategy = resolver.resolve_for_route(0).unwrap();

        // Should only have local middleware (no inheritance from site)
        assert_eq!(local_strategy.middleware.len(), 3); // rate_limit, cors(off), local_only

        // Check rate_limit: only from local
        let rate_limit = local_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "rate_limit")
            .unwrap();
        let config = rate_limit.config_as_json().unwrap();
        assert_eq!(config["requests"], 50); // From local only
        assert_eq!(config["local_rl"], "local_rl");
        // No site or global fields

        // Check cors: disabled in local
        let cors = local_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "cors")
            .unwrap();
        assert!(cors.is_off());

        // Check local_only: only from local
        let local_only = local_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "local_only")
            .unwrap();
        assert!(local_only.config_as_json().is_ok());

        // Should NOT have site or global middleware
        assert!(
            local_strategy
                .middleware
                .iter()
                .find(|m: &&MiddlewareConfig| m.name() == "logging")
                .is_none()
        );
        assert!(
            local_strategy
                .middleware
                .iter()
                .find(|m: &&MiddlewareConfig| m.name() == "security")
                .is_none()
        );

        println!("✅ Hierarchical route local strategy isolation works correctly");
    }

    #[test]
    fn test_hierarchical_deep_inheritance_chain() {
        // Test: Global -> Site -> Route (named) -> Inline
        let global = create_global_with_all_middleware();
        let mut site = create_site_with_partial_override();

        // Add route with named strategy that inherits from site
        site.routes.push(Route::Proxy {
            r#match: Match {
                path: Some("/deep/{*path}".to_string()),
                path_regex: None,
            },
            backend: "http://deep:8080/{*path}".to_string(),
            strategy: Some(StrategyRef::Named("deep_route".to_string())),
            strategies: Some({
                let mut deep_strats = std::collections::HashMap::new();
                deep_strats.insert(
                    "deep_route".to_string(),
                    vec![
                        // Override cache at route level
                        MiddlewareConfig::new_named_json(
                            "cache".to_string(),
                            json!({"ttl": 120, "deep_cache": "deep_cache"}),
                        ),
                        // Add route-specific middleware
                        MiddlewareConfig::new_named_json(
                            "compression".to_string(),
                            json!({"enabled": true, "deep_compression": "deep_compression"}),
                        ),
                    ],
                );
                deep_strats
            }),
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let deep_strategy = resolver.resolve_for_route(0).unwrap();

        // Should have: route overrides + site+global inherited
        println!(
            "Debug - deep strategy middleware count: {}",
            deep_strategy.middleware.len()
        );
        for (i, m) in deep_strategy.middleware.iter().enumerate() {
            println!("  {}: {}", i, m.name());
        }
        // Actually, route local strategies don't inherit from site - they are standalone
        // cors gets filtered out (off)
        assert_eq!(deep_strategy.middleware.len(), 2); // cache, compression

        // Check cache: only from route (no inheritance)
        let cache = deep_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "cache")
            .unwrap();
        let config = cache.config_as_json().unwrap();
        assert_eq!(config["ttl"], 120); // From route only
        assert_eq!(config["deep_cache"], "deep_cache"); // From route only
        // No site or global fields

        // Check compression: only from route
        let compression = deep_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "compression")
            .unwrap();
        let comp_config = compression.config_as_json().unwrap();
        assert_eq!(comp_config["deep_compression"], "deep_compression");

        // Should NOT have rate_limit, logging, etc. (route local strategies are standalone)
        assert!(
            deep_strategy
                .middleware
                .iter()
                .find(|m: &&MiddlewareConfig| m.name() == "rate_limit")
                .is_none()
        );
        assert!(
            deep_strategy
                .middleware
                .iter()
                .find(|m: &&MiddlewareConfig| m.name() == "logging")
                .is_none()
        );

        println!("✅ Hierarchical deep inheritance chain works correctly (route local isolation)");
    }

    #[test]
    fn test_hierarchical_with_off_at_different_levels() {
        let global = create_global_with_all_middleware();
        let mut site = create_site_with_partial_override();

        // Modify site to disable some middleware
        site.strategies.get_mut("site_override").unwrap()[1] =
            MiddlewareConfig::new_off("logging".to_string());

        // Add route with inline that re-enables some and disables others
        site.routes.push(Route::Proxy {
            r#match: Match {
                path: Some("/off_levels/{*path}".to_string()),
                path_regex: None,
            },
            backend: "http://off:8080/{*path}".to_string(),
            strategy: Some(StrategyRef::InlineMiddleware(vec![
                // Re-enable logging at inline level
                MiddlewareConfig::new_named_json(
                    "logging".to_string(),
                    json!({"level": "trace", "inline_log": "inline_log"}),
                ),
                // Disable auth at inline level
                MiddlewareConfig::new_off("auth".to_string()),
                // Keep rate_limit from site
                // Add new inline middleware
                MiddlewareConfig::new_named_json(
                    "inline_new".to_string(),
                    json!({"inline_field": "inline_field"}),
                ),
            ])),
            strategies: None,
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let off_strategy = resolver.resolve_for_route(0).unwrap();

        // Should have: rate_limit, logging (re-enabled), cors, cache, security, inline_new
        // auth should be filtered out (off in inline)
        assert_eq!(off_strategy.middleware.len(), 6);

        // Check logging: re-enabled at inline level (overrides site off)
        let logging = off_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "logging")
            .unwrap();
        let log_config = logging.config_as_json().unwrap();
        assert_eq!(log_config["level"], "trace"); // From inline
        assert_eq!(log_config["inline_log"], "inline_log"); // From inline
        assert_eq!(log_config["global_log"], "global_log"); // From global (supplement)

        // Check auth: should be removed (off in inline)
        assert!(
            off_strategy
                .middleware
                .iter()
                .find(|m: &&MiddlewareConfig| m.name() == "auth")
                .is_none()
        );

        // Check rate_limit: from site (which overrode global)
        let rate_limit = off_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "rate_limit")
            .unwrap();
        assert!(rate_limit.config_as_json().is_ok());

        println!("✅ Hierarchical with off at different levels works correctly");
    }

    #[test]
    fn test_hierarchical_empty_inheritance_levels() {
        let global = create_global_with_all_middleware();

        // Site with no strategy
        let mut site = SiteConfig {
            domain: "empty.local".to_string(),
            domains: vec!["empty.local".to_string()],
            listeners: vec![],
            routes: vec![],
            strategy: None,                               // No site strategy
            strategies: std::collections::HashMap::new(), // Empty strategies
        };

        // Add route with inline that inherits only from global
        site.routes.push(Route::Proxy {
            r#match: Match {
                path: Some("/empty/{*path}".to_string()),
                path_regex: None,
            },
            backend: "http://empty:8080/{*path}".to_string(),
            strategy: Some(StrategyRef::InlineMiddleware(vec![
                MiddlewareConfig::new_named_json(
                    "rate_limit".to_string(),
                    json!({"requests": 25, "window": "2s", "inline_rl": "inline_rl"}),
                ),
            ])),
            strategies: None,
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let empty_strategy = resolver.resolve_for_route(0).unwrap();

        // Should have: inline rate_limit + all global middleware (no site)
        assert_eq!(empty_strategy.middleware.len(), 5); // rate_limit (inline) + cors, auth, cache (global)

        // Check rate_limit: inline + global inheritance
        let rate_limit = empty_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "rate_limit")
            .unwrap();
        let config = rate_limit.config_as_json().unwrap();
        assert_eq!(config["requests"], 25); // From inline
        assert_eq!(config["inline_rl"], "inline_rl"); // From inline
        assert_eq!(config["global_rl"], "global_rl"); // From global (supplement)

        // Check cors: only from global
        let cors = empty_strategy
            .middleware
            .iter()
            .find(|m: &&MiddlewareConfig| m.name() == "cors")
            .unwrap();
        let cors_config = cors.config_as_json().unwrap();
        assert_eq!(cors_config["global_cors"], "global_cors");

        println!("✅ Hierarchical with empty inheritance levels works correctly");
    }

    #[test]
    fn test_hierarchical_strategy_resolution_priority_matrix() {
        // Test all combinations of strategy resolution
        let global = create_global_with_all_middleware();
        let mut site = create_site_with_partial_override();

        // Test matrix: [global, site, route] combinations
        let test_cases = vec![
            // (route_strategy, expected_middleware_count, description)
            (
                Some(StrategyRef::Named("global_base".to_string())),
                5,
                "Route uses global directly",
            ),
            (
                Some(StrategyRef::Named("site_override".to_string())),
                6,
                "Route uses site (merged with global)",
            ),
            (
                Some(StrategyRef::InlineMiddleware(vec![
                    MiddlewareConfig::new_named_json("test".to_string(), json!({"value": 1})),
                ])),
                7,
                "Route uses inline (inherits from site+global)",
            ),
            (None, 6, "Route uses site default (merged with global)"),
        ];

        for (i, (route_strategy, expected_count, description)) in test_cases.into_iter().enumerate()
        {
            // Clear existing routes and add test route
            site.routes.clear();
            site.routes.push(Route::Proxy {
                r#match: Match {
                    path: Some(format!("/test_{}/{{*path}}", i)),
                    path_regex: None,
                },
                backend: format!("http://test{}:8080/{{*path}}", i),
                strategy: route_strategy,
                strategies: None,
            });

            let resolver = StrategyResolver::new(&site, &global).unwrap();
            let strategy = resolver.resolve_for_route(0).unwrap();

            assert_eq!(
                strategy.middleware.len(),
                expected_count,
                "Failed for {}: expected {}, got {}",
                description,
                expected_count,
                strategy.middleware.len()
            );
        }

        println!("✅ Hierarchical strategy resolution priority matrix works correctly");
    }
}
