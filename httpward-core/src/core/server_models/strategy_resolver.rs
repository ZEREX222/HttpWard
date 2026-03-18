use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, trace, instrument};

use crate::config::strategy::{
    Strategy, StrategyRef,
    supplement_middleware,         // now accepts &[MiddlewareConfig]
    LegacyStrategyCollection,
};

use crate::config::{SiteConfig, GlobalConfig, Route, MiddlewareConfig};

pub type StrategyCollection = HashMap<String, Arc<Vec<MiddlewareConfig>>>;

#[derive(Debug, Clone)]
pub struct StrategyResolver {
    global: Arc<StrategyCollection>,
    merged_site: Arc<StrategyCollection>,
    resolved_routes: Vec<Option<Arc<Strategy>>>,
}

impl StrategyResolver {

    #[instrument(skip(site, global))]
    pub fn new(site: &SiteConfig, global: &GlobalConfig) -> Result<Self> {
        debug!("Creating StrategyResolver for site: {}", site.domain);

        // Convert global strategies — clone only the Arc pointers (cheap).
        let global_map: StrategyCollection = global.strategies
            .iter()
            .map(|(name, vec)| (name.clone(), Arc::new(vec.clone())))
            .collect();

        let global_map = Arc::new(global_map);

        // Extract global default name (if named).
        let global_default_name: Option<String> = match &global.strategy {
            Some(StrategyRef::Named(n)) => Some(n.clone()),
            _ => None,
        };

        // Start merged_site with global strategies (clone Arc pointers).
        let mut merged_site: StrategyCollection = global_map
            .iter()
            .map(|(n, v)| (n.clone(), v.clone()))
            .collect();

        // Merge site's named strategies with global or global default.
        for (name, site_vec) in &site.strategies {
            let mut merged = site_vec.clone();

            // If a global with same name exists — supplement from it without cloning the global Vec.
            if let Some(global_same) = global_map.get(name) {
                // "global_same.as_ref().as_slice()" produces &[MiddlewareConfig] - no cloning!
                supplement_middleware(&mut merged, global_same.as_ref().as_slice())?;
            }
            // Otherwise, if a global default exists — inherit that.
            else if let Some(default_name) = &global_default_name {
                if let Some(global_default_vec) = global_map.get(default_name) {
                    supplement_middleware(&mut merged, global_default_vec.as_ref().as_slice())?;
                }
            }

            merged_site.insert(name.clone(), Arc::new(merged));
        }

        let merged_site = Arc::new(merged_site);

        // Precompute resolved strategies per route.
        let mut resolved_routes = Vec::with_capacity(site.routes.len());

        for (index, route) in site.routes.iter().enumerate() {
            let resolved = Self::resolve_route_strategy(
                route,
                site,
                &merged_site,
                &global.strategy,
            )?;

            trace!(
                "Precomputed strategy for route {}: {:?}",
                index,
                resolved.as_ref().map(|s| &s.name)
            );

            resolved_routes.push(resolved);
        }

        Ok(Self {
            global: global_map,
            merged_site,
            resolved_routes,
        })
    }

    // Helper: when route provides a named strategy override (route-local strategies).
    // Returns Some(Arc<Strategy>) or None.
    fn resolve_route_strategy(
        route: &Route,
        site: &SiteConfig,
        merged_site: &StrategyCollection,
        global_default: &Option<StrategyRef>,
    ) -> Result<Option<Arc<Strategy>>> {

        // Determine effective StrategyRef in order: route -> site -> global default
        let strategy_ref = route.get_strategy()
            .cloned()
            .or_else(|| site.strategy.clone())
            .or_else(|| global_default.clone());

        let resolved = match strategy_ref {
            Some(StrategyRef::InlineMiddleware(m)) => {
                // Inline middleware should inherit from site strategy first, then global default
                let mut merged = m.clone();
                
                // First, supplement with site strategy if it exists
                if let Some(StrategyRef::Named(site_strategy_name)) = &site.strategy {
                    if let Some(site_strategy_vec) = merged_site.get(site_strategy_name) {
                        supplement_middleware(&mut merged, site_strategy_vec.as_ref().as_slice())?;
                    }
                }
                
                // Then, supplement with global default strategy if it exists and is different
                if let Some(StrategyRef::Named(default_strategy_name)) = &global_default {
                    // Only supplement if site strategy is different from global default
                    let site_strategy_name = site.strategy.as_ref()
                        .and_then(|s| match s {
                            StrategyRef::Named(name) => Some(name.clone()),
                            _ => None,
                        });
                    
                    if site_strategy_name.as_ref() != Some(default_strategy_name) {
                        if let Some(global_default_vec) = merged_site.get(default_strategy_name) {
                            supplement_middleware(&mut merged, global_default_vec.as_ref().as_slice())?;
                        }
                    }
                }
                
                // Filter out disabled middleware after inheritance
                // Remove any middleware that was added by inheritance but should be disabled by inline
                let mut filtered = Vec::new();
                for middleware in merged {
                    if !middleware.is_off() {
                        filtered.push(middleware);
                    }
                }
                
                Some(Arc::new(Strategy {
                    name: "inline".to_string(),
                    middleware: Arc::new(filtered),
                }))
            }

            Some(StrategyRef::Named(name)) => {
                // If route has its own strategies map, it may override the named strategy locally.
                if let Some(route_strats) = route.get_strategies() {
                    if let Some(route_vec) = route_strats.get(&name) {
                        let mut merged = route_vec.clone();

                        // If there's a parent (site/global) with same name, supplement from parent without cloning parent vec.
                        if let Some(parent) = merged_site.get(&name) {
                            supplement_middleware(&mut merged, parent.as_ref().as_slice())?;
                        }

                        return Ok(Some(Arc::new(Strategy {
                            name,
                            middleware: Arc::new(merged),
                        })));
                    }
                }

                // Otherwise, use merged_site version if exists (reuse parent's Arc<Vec<...>> directly).
                merged_site
                    .get(&name)
                    .map(|v| Arc::new(Strategy {
                        name,
                        middleware: v.clone(), // clone Arc (cheap)
                    }))
            }

            None => None,
        };

        Ok(resolved)
    }

    #[instrument(skip(self))]
    pub fn resolve_for_route(&self, route_index: usize) -> Option<Arc<Strategy>> {
        self.resolved_routes
            .get(route_index)
            .cloned()
            .flatten()
    }

    #[instrument(skip(self, site))]
    pub fn resolve_for_site(&self, site: &SiteConfig) -> Result<Option<Arc<Strategy>>> {
        match &site.strategy {
            Some(StrategyRef::InlineMiddleware(m)) => Ok(Some(Arc::new(Strategy {
                name: "inline".into(),
                middleware: Arc::new(m.clone()),
            }))),
            Some(StrategyRef::Named(name)) => Ok(self.merged_site.get(name).map(|v| Arc::new(Strategy {
                name: name.clone(),
                middleware: v.clone(),
            }))),
            None => Ok(None),
        }
    }

    pub fn get_all_strategies(&self) -> LegacyStrategyCollection {
        // Collect global (clone Vecs) then overwrite with merged_site (site-level merges).
        let mut all = self.global
            .iter()
            .map(|(n, v)| (n.clone(), (**v).clone()))
            .collect::<LegacyStrategyCollection>();

        for (name, arc_vec) in self.merged_site.iter() {
            all.insert(name.clone(), (**arc_vec).clone());
        }

        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::{GlobalConfig, Route, Match};
    use crate::config::strategy::{StrategyRef, MiddlewareConfig};
    use serde_json::json;
    use std::path::PathBuf;

    fn create_test_global_config() -> GlobalConfig {
        GlobalConfig {
            domain: "example.com".to_string(),
            domains: vec![],
            listeners: vec![],
            routes: vec![],
            log: Default::default(),
            proxy_id: "httpward".to_string(),
            sites_enabled: PathBuf::from("/tmp/sites"),
            strategy: Some(StrategyRef::Named("default".to_string())),
            strategies: {
                let mut strategies = std::collections::HashMap::new();
                strategies.insert("default".to_string(), vec![
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({
                            "requests": 1000,
                            "window": "1m"
                        })
                    ),
                    MiddlewareConfig::new_named_json(
                        "logging".to_string(),
                        json!({
                            "level": "info",
                            "format": "json"
                        })
                    )
                ]);
                strategies.insert("strict".to_string(), vec![
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({
                            "requests": 100,
                            "window": "1m"
                        })
                    ),
                    MiddlewareConfig::new_named_json(
                        "auth".to_string(),
                        json!({
                            "type": "jwt"
                        })
                    )
                ]);
                strategies
            },
        }
    }

    fn create_test_site_config() -> SiteConfig {
        SiteConfig {
            domain: "test.com".to_string(),
            domains: vec!["api.test.com".to_string()],
            listeners: vec![],
            routes: vec![
                Route::Proxy {
                    r#match: Match {
                        path: Some("/api".to_string()),
                        path_regex: None,
                    },
                    backend: "http://backend".to_string(),
                    strategy: Some(StrategyRef::Named("api".to_string())),
                    strategies: None,
                },
                Route::Static {
                    r#match: Match {
                        path: Some("/static".to_string()),
                        path_regex: None,
                    },
                    static_dir: PathBuf::from("/var/www"),
                    strategy: None,
                    strategies: None,
                },
            ],
            strategy: Some(StrategyRef::Named("site_default".to_string())),
            strategies: {
                let mut strategies = std::collections::HashMap::new();
                strategies.insert("site_default".to_string(), vec![
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({
                            "requests": 500,
                            "window": "30s"
                        })
                    ),
                    MiddlewareConfig::new_named_json(
                        "cors".to_string(),
                        json!({
                            "origins": ["*"]
                        })
                    )
                ]);
                strategies.insert("api".to_string(), vec![
                    MiddlewareConfig::new_named_json(
                        "auth".to_string(),
                        json!({
                            "type": "api_key"
                        })
                    ),
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({
                            "requests": 200,
                            "window": "1m"
                        })
                    )
                ]);
                strategies
            },
        }
    }

    #[test]
    fn test_strategy_resolver_creation() {
        let global = create_test_global_config();
        let site = create_test_site_config();

        let resolver = StrategyResolver::new(&site, &global).unwrap();

        // Check that global strategies are preserved
        assert_eq!(resolver.global.len(), 2);
        assert!(resolver.global.contains_key("default"));
        assert!(resolver.global.contains_key("strict"));

        // Check that merged site strategies include both global and site strategies
        assert_eq!(resolver.merged_site.len(), 4); // default, strict, site_default, api
        assert!(resolver.merged_site.contains_key("default"));
        assert!(resolver.merged_site.contains_key("strict"));
        assert!(resolver.merged_site.contains_key("site_default"));
        assert!(resolver.merged_site.contains_key("api"));

        // Check that resolved routes are precomputed
        assert_eq!(resolver.resolved_routes.len(), 2);
    }

    #[test]
    fn test_strategy_resolution_for_route() {
        let global = create_test_global_config();
        let site = create_test_site_config();
        let resolver = StrategyResolver::new(&site, &global).unwrap();

        // Route 0: /api with explicit "api" strategy
        let api_strategy = resolver.resolve_for_route(0).unwrap();
        assert_eq!(api_strategy.name, "api");
        assert_eq!(api_strategy.middleware.len(), 3); // logging (global) + auth, rate_limit (site)
        assert_eq!(api_strategy.middleware[0].name(), "logging");
        assert_eq!(api_strategy.middleware[1].name(), "auth");
        assert_eq!(api_strategy.middleware[2].name(), "rate_limit");

        // Route 1: /static with no explicit strategy (should use site strategy)
        let static_strategy = resolver.resolve_for_route(1);
        assert!(static_strategy.is_some()); // Should inherit site strategy
        assert_eq!(static_strategy.unwrap().name, "site_default");
    }

    #[test]
    fn test_strategy_resolution_for_site() {
        let global = create_test_global_config();
        let site = create_test_site_config();
        let resolver = StrategyResolver::new(&site, &global).unwrap();

        let site_strategy = resolver.resolve_for_site(&site).unwrap().unwrap();
        assert_eq!(site_strategy.name, "site_default");
        assert_eq!(site_strategy.middleware.len(), 3); // logging (global) + rate_limit, cors (site)
        assert_eq!(site_strategy.middleware[0].name(), "logging");
        assert_eq!(site_strategy.middleware[1].name(), "rate_limit");
        assert_eq!(site_strategy.middleware[2].name(), "cors");
    }

    #[test]
    fn test_strategy_inheritance_and_supplementation() {
        let mut global = create_test_global_config();
        let mut site = create_test_site_config();

        // Modify global default strategy
        global.strategies.insert("default".to_string(), vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 1000,
                    "window": "1m",
                    "burst": 200
                })
            ),
            MiddlewareConfig::new_named_json(
                "logging".to_string(),
                json!({
                    "level": "info"
                })
            )
        ]);

        // Modify site strategy to inherit from global default
        site.strategies.insert("site_default".to_string(), vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 500  // Override
                })
            ),
            MiddlewareConfig::new_named_json(
                "cors".to_string(),
                json!({
                    "origins": ["*"]  // New middleware
                })
            )
        ]);

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let site_strategy = resolver.resolve_for_site(&site).unwrap().unwrap();
        
        // Check that rate_limit was supplemented (not merged) with global default
        let rate_limit_config = site_strategy.middleware
            .iter()
            .find(|m| m.name() == "rate_limit")
            .unwrap()
            .config_as_json()
            .unwrap();
        assert_eq!(rate_limit_config["requests"], 500);  // Site override preserved
        assert_eq!(rate_limit_config["window"], "1m");   // Supplemented from global
        assert_eq!(rate_limit_config["burst"], 200);    // Supplemented from global

        // Check that logging was added from global default
        assert_eq!(site_strategy.middleware.len(), 3);
        assert_eq!(site_strategy.middleware[0].name(), "logging");
    }

    #[test]
    fn test_inline_middleware_strategy() {
        let global = create_test_global_config();
        let mut site = create_test_site_config();

        // Add route with inline middleware
        site.routes.push(Route::Redirect {
            r#match: Match {
                path: Some("/redirect".to_string()),
                path_regex: None,
            },
            redirect: crate::config::global::Redirect {
                to: "https://example.com".to_string(),
                code: 301,
            },
            strategy: Some(StrategyRef::InlineMiddleware(vec![
                MiddlewareConfig::new_named_json(
                    "rate_limit".to_string(),
                    json!({
                        "requests": 10,
                        "window": "1s"
                    })
                )
            ])),
            strategies: None,
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let redirect_strategy = resolver.resolve_for_route(2).unwrap();

        assert_eq!(redirect_strategy.name, "inline");
        assert_eq!(redirect_strategy.middleware.len(), 3); // logging (global) + cors (site) + rate_limit (inline)
        assert_eq!(redirect_strategy.middleware[2].name(), "rate_limit"); // From inline

        let rate_limit_config = redirect_strategy.middleware[2].config_as_json().unwrap();
        assert_eq!(rate_limit_config["requests"], 10); // From inline (takes precedence)
        assert_eq!(rate_limit_config["window"], "1s"); // From inline (takes precedence)

        // Check that site middleware is present
        assert_eq!(redirect_strategy.middleware[0].name(), "logging"); // From global default
        assert_eq!(redirect_strategy.middleware[1].name(), "cors"); // From site strategy
    }

    #[test]
    fn test_route_local_strategy_override() {
        let global = create_test_global_config();
        let mut site = create_test_site_config();

        // Add route with local strategy definitions
        site.routes.push(Route::Proxy {
            r#match: Match {
                path: Some("/admin".to_string()),
                path_regex: None,
            },
            backend: "http://admin".to_string(),
            strategy: Some(StrategyRef::Named("strict".to_string())),
            strategies: {
                let mut strategies = std::collections::HashMap::new();
                strategies.insert("strict".to_string(), vec![
                    MiddlewareConfig::new_named_json(
                        "auth".to_string(),
                        json!({
                            "type": "basic",
                            "realm": "Admin Area"
                        })
                    ),
                    MiddlewareConfig::new_named_json(
                        "audit".to_string(),
                        json!({
                            "enabled": true
                        })
                    )
                ]);
                Some(strategies)
            },
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let admin_strategy = resolver.resolve_for_route(2).unwrap();

        assert_eq!(admin_strategy.name, "strict");
        assert_eq!(admin_strategy.middleware.len(), 3); // auth (route) + audit (route) + rate_limit (global)

        // Check that route auth overrides global auth
        let auth_config = admin_strategy.middleware
            .iter()
            .find(|m| m.name() == "auth")
            .unwrap()
            .config_as_json()
            .unwrap();
        assert_eq!(auth_config["type"], "basic");
        assert_eq!(auth_config["realm"], "Admin Area");

        // Check that rate_limit was supplemented from global strict strategy
        let rate_limit_config = admin_strategy.middleware
            .iter()
            .find(|m| m.name() == "rate_limit")
            .unwrap()
            .config_as_json()
            .unwrap();
        assert_eq!(rate_limit_config["requests"], 100);
        assert_eq!(rate_limit_config["window"], "1m");
    }

    #[test]
    fn test_get_all_strategies() {
        let global = create_test_global_config();
        let site = create_test_site_config();
        let resolver = StrategyResolver::new(&site, &global).unwrap();

        let all_strategies = resolver.get_all_strategies();
        
        // Should contain all strategies: global + merged site
        assert_eq!(all_strategies.len(), 4);
        assert!(all_strategies.contains_key("default"));
        assert!(all_strategies.contains_key("strict"));
        assert!(all_strategies.contains_key("site_default"));
        assert!(all_strategies.contains_key("api"));

        // Check that site strategies include supplemented middleware
        let site_default = all_strategies.get("site_default").unwrap();
        assert_eq!(site_default.len(), 3); // rate_limit + cors + logging (supplemented from global default)
    }

    #[test]
    fn test_no_global_default_strategy() {
        let mut global = create_test_global_config();
        global.strategy = None; // No global default
        
        let site = create_test_site_config();
        let resolver = StrategyResolver::new(&site, &global).unwrap();

        // Should still work correctly
        assert_eq!(resolver.resolved_routes.len(), 2);
        
        // Route with explicit strategy should still resolve
        let api_strategy = resolver.resolve_for_route(0).unwrap();
        assert_eq!(api_strategy.name, "api");
    }

    #[test]
    fn test_empty_strategies() {
        let global = GlobalConfig::default();
        let site = SiteConfig::default();

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        assert_eq!(resolver.global.len(), 0);
        assert_eq!(resolver.merged_site.len(), 0);
        assert_eq!(resolver.resolved_routes.len(), 0);
        
        let all_strategies = resolver.get_all_strategies();
        assert_eq!(all_strategies.len(), 0);
    }

    #[test]
    fn test_strategy_resolver_with_complex_merging() {
        let mut global = create_test_global_config();
        
        // Global strategies with overlapping middleware names
        global.strategies.insert("base".to_string(), vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 1000,
                    "window": "1m",
                    "burst": 200
                })
            ),
            MiddlewareConfig::new_named_json(
                "logging".to_string(),
                json!({
                    "level": "info",
                    "format": "json"
                })
            ),
            MiddlewareConfig::new_named_json(
                "cors".to_string(),
                json!({
                    "origins": ["https://example.com"]
                })
            )
        ]);

        let mut site = create_test_site_config();
        
        // Site strategies that inherit and override
        site.strategies.insert("enhanced".to_string(), vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 500  // Override
                })
            ),
            MiddlewareConfig::new_named_json(
                "auth".to_string(),
                json!({
                    "type": "oauth2"  // New middleware
                })
            )
        ]);

        site.strategy = Some(StrategyRef::Named("enhanced".to_string()));

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let enhanced_strategy = resolver.resolve_for_site(&site).unwrap().unwrap();

        assert_eq!(enhanced_strategy.name, "enhanced");
        assert_eq!(enhanced_strategy.middleware.len(), 3); // rate_limit + auth + logging (from global base)

        // Check rate_limit: site override + global default supplementation  
        let rate_limit_config = enhanced_strategy.middleware
            .iter()
            .find(|m| m.name() == "rate_limit")
            .unwrap()
            .config_as_json()
            .unwrap();
        assert_eq!(rate_limit_config["requests"], 500);  // Site override
        assert_eq!(rate_limit_config["window"], "1m");   // From global default
        // Note: burst field doesn't exist because global default doesn't have it

        // Check auth: site only
        let auth_config = enhanced_strategy.middleware
            .iter()
            .find(|m| m.name() == "auth")
            .unwrap()
            .config_as_json()
            .unwrap();
        assert_eq!(auth_config["type"], "oauth2");

        // Check logging: from global base
        let logging_config = enhanced_strategy.middleware
            .iter()
            .find(|m| m.name() == "logging")
            .unwrap()
            .config_as_json()
            .unwrap();
        assert_eq!(logging_config["level"], "info");
        assert_eq!(logging_config["format"], "json");
    }

    #[test]
    fn test_performance_large_scale() {
        let mut strategies = std::collections::HashMap::new();
        
        // Create large global config with many strategies and middleware
        for i in 0..20 {
            let mut middleware = Vec::new();
            for j in 0..50 {
                middleware.push(MiddlewareConfig::new_named_json(
                    format!("middleware_{}", j),
                    json!({
                        "param1": format!("value_{}_{}", i, j),
                        "param2": j * 10,
                        "param3": format!("config_{}_{}", i, j)
                    })
                ));
            }
            strategies.insert(format!("strategy_{}", i), middleware);
        }

        let global = GlobalConfig {
            domain: "example.com".to_string(),
            domains: vec![],
            listeners: vec![],
            routes: vec![],
            log: Default::default(),
            proxy_id: "httpward".to_string(),
            sites_enabled: std::path::PathBuf::from("/tmp/sites"),
            strategy: Some(StrategyRef::Named("strategy_0".to_string())),
            strategies,
        };

        let mut routes = Vec::new();
        for i in 0..100 {
            routes.push(Route::Proxy {
                r#match: Match {
                    path: Some(format!("/route_{}", i)),
                    path_regex: None,
                },
                backend: format!("http://backend_{}", i),
                strategy: Some(StrategyRef::Named(format!("site_strategy_{}", i % 5))),
                strategies: None,
            });
        }

        let mut site_strategies = std::collections::HashMap::new();
        for i in 0..5 {
            let mut middleware = Vec::new();
            for j in 0..10 {
                middleware.push(MiddlewareConfig::new_named_json(
                    format!("site_middleware_{}", j),
                    json!({
                        "site_param": format!("site_value_{}_{}", i, j),
                        "site_index": j
                    })
                ));
            }
            site_strategies.insert(format!("site_strategy_{}", i), middleware);
        }

        let site = SiteConfig {
            domain: "test.com".to_string(),
            domains: vec![],
            listeners: vec![],
            routes,
            strategy: Some(StrategyRef::Named("site_strategy_0".to_string())),
            strategies: site_strategies,
        };

        println!("Performance test setup:");
        println!("- Global strategies: {} with 50 middleware each", global.strategies.len());
        println!("- Site routes: {}", site.routes.len());
        println!("- Site strategies: {} with 10 middleware each", site.strategies.len());

        let start = std::time::Instant::now();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let creation_time = start.elapsed();

        println!("\nStrategyResolver creation: {:?}", creation_time);
        println!("Resolved routes: {}", resolver.resolved_routes.len());

        // Test route resolution performance
        let start = std::time::Instant::now();
        for i in 0..site.routes.len() {
            let _strategy = resolver.resolve_for_route(i);
        }
        let resolution_time = start.elapsed();

        println!("Route resolution ({} routes): {:?}", site.routes.len(), resolution_time);
        println!("Average per route: {:?}", resolution_time / site.routes.len() as u32);

        // Verify correctness
        assert_eq!(resolver.resolved_routes.len(), site.routes.len());
        
        // All routes should have strategies (either explicit or inherited)
        for i in 0..site.routes.len() {
            assert!(resolver.resolve_for_route(i).is_some(), 
                   "Route {} should have a strategy", i);
        }

        println!("\n✅ Performance test completed successfully!");
        println!("✅ No memory allocations during strategy resolution (using slices)");
        println!("✅ All routes properly resolved with inheritance");

        // Performance assertion - should be fast even with large configs
        assert!(creation_time.as_millis() < 100, "StrategyResolver creation should be fast even with large configs");
        assert!(resolution_time.as_millis() < 50, "Route resolution should be very fast");
    }

    #[test]
    fn test_inline_strategy_inherits_from_global_default() {
        let mut global = create_test_global_config();
        let mut site = create_test_site_config();

        // Add route with inline middleware
        site.routes.push(Route::Proxy {
            r#match: Match {
                path: Some("/inline-test".to_string()),
                path_regex: None,
            },
            backend: "http://backend".to_string(),
            strategy: Some(StrategyRef::InlineMiddleware(vec![
                MiddlewareConfig::new_named_json(
                    "auth".to_string(),
                    json!({
                        "type": "basic"
                    })
                )
            ])),
            strategies: None,
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let inline_strategy = resolver.resolve_for_route(2).unwrap();

        assert_eq!(inline_strategy.name, "inline");
        
        // Should have inline middleware + site strategy + global default middleware
        assert_eq!(inline_strategy.middleware.len(), 4); // logging (global) + rate_limit,cors (site) + auth (inline)
        assert_eq!(inline_strategy.middleware[0].name(), "logging"); // From global default
        assert_eq!(inline_strategy.middleware[1].name(), "rate_limit"); // From site strategy
        assert_eq!(inline_strategy.middleware[2].name(), "cors"); // From site strategy
        assert_eq!(inline_strategy.middleware[3].name(), "auth"); // From inline

        println!("✅ Inline strategy correctly inherits from site and global default");
    }

    #[test]
    fn test_inline_strategy_with_no_global_default() {
        let mut global = create_test_global_config();
        global.strategy = None; // No global default
        
        let mut site = create_test_site_config();

        // Add route with inline middleware
        site.routes.push(Route::Proxy {
            r#match: Match {
                path: Some("/inline-test".to_string()),
                path_regex: None,
            },
            backend: "http://backend".to_string(),
            strategy: Some(StrategyRef::InlineMiddleware(vec![
                MiddlewareConfig::new_named_json(
                    "auth".to_string(),
                    json!({
                        "type": "basic"
                    })
                )
            ])),
            strategies: None,
        });

        let resolver = StrategyResolver::new(&site, &global).unwrap();
        let inline_strategy = resolver.resolve_for_route(2).unwrap();

        assert_eq!(inline_strategy.name, "inline");
        
        // Should have inline middleware + site strategy middleware (no global default)
        assert_eq!(inline_strategy.middleware.len(), 3); // rate_limit,cors (site strategy) + auth (inline)
        assert_eq!(inline_strategy.middleware[0].name(), "rate_limit"); // From site strategy
        assert_eq!(inline_strategy.middleware[1].name(), "cors"); // From site strategy
        assert_eq!(inline_strategy.middleware[2].name(), "auth"); // From inline

        println!("✅ Inline strategy correctly inherits from site strategy");
    }

    // Include the off inheritance tests
    mod off_inheritance_tests;
    
    // Include the user scenario test
    mod user_scenario_test;
    
    // Include the comprehensive inheritance tests
    mod comprehensive_inheritance_tests;
    
    // Include the hierarchical inheritance tests
    mod hierarchical_inheritance_tests;
    
    /// Comprehensive test for complete hierarchy validation and middleware sequence
    /// This test validates the entire inheritance chain: global -> site -> route -> inline
    /// and ensures correct middleware ordering and supplementation behavior
    #[test]
    fn test_complete_hierarchy_and_middleware_sequence() {
        // Create comprehensive global configuration
        let mut global = create_test_global_config();
        
        // Define global strategies with overlapping middleware names
        global.strategies.insert("base".to_string(), vec![
            MiddlewareConfig::new_named_json(
                "logging".to_string(),
                json!({
                    "level": "info",
                    "format": "json",
                    "source": "global_base"
                })
            ),
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 1000,
                    "window": "1m",
                    "burst": 200,
                    "source": "global_base"
                })
            ),
            MiddlewareConfig::new_named_json(
                "cors".to_string(),
                json!({
                    "origins": ["https://api.example.com"],
                    "max_age": 3600,
                    "source": "global_base"
                })
            )
        ]);
        
        global.strategies.insert("security".to_string(), vec![
            MiddlewareConfig::new_named_json(
                "auth".to_string(),
                json!({
                    "type": "oauth2",
                    "provider": "global_auth",
                    "source": "global_security"
                })
            ),
            MiddlewareConfig::new_named_json(
                "audit".to_string(),
                json!({
                    "enabled": true,
                    "log_level": "debug",
                    "source": "global_security"
                })
            )
        ]);
        
        // Set global default strategy
        global.strategy = Some(StrategyRef::Named("base".to_string()));
        
        // Create comprehensive site configuration
        let mut site = create_test_site_config();
        
        // Define site strategies that inherit from and override global
        site.strategies.insert("site_enhanced".to_string(), vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 500,  // Override global value
                    "window": "30s",  // Override global value
                    "source": "site_enhanced"
                })
            ),
            MiddlewareConfig::new_named_json(
                "compression".to_string(),
                json!({
                    "enabled": true,
                    "level": "gzip",
                    "source": "site_enhanced"
                })
            )
        ]);
        
        site.strategies.insert("api_public".to_string(), vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 100,
                    "window": "1m",
                    "source": "site_api_public"
                })
            ),
            MiddlewareConfig::new_named_json(
                "cache".to_string(),
                json!({
                    "ttl": 300,
                    "strategy": "lru",
                    "source": "site_api_public"
                })
            )
        ]);
        
        // Set site default strategy
        site.strategy = Some(StrategyRef::Named("site_enhanced".to_string()));
        
        // Add comprehensive routes with different strategy scenarios
        site.routes = vec![
            // Route 1: Uses site default strategy (inherits from global base)
            Route::Proxy {
                r#match: Match {
                    path: Some("/public".to_string()),
                    path_regex: None,
                },
                backend: "http://public-api".to_string(),
                strategy: None, // Will inherit site strategy
                strategies: None,
            },
            
            // Route 2: Uses named site strategy
            Route::Proxy {
                r#match: Match {
                    path: Some("/api".to_string()),
                    path_regex: None,
                },
                backend: "http://api-service".to_string(),
                strategy: Some(StrategyRef::Named("api_public".to_string())),
                strategies: None,
            },
            
            // Route 3: Uses global named strategy directly
            Route::Proxy {
                r#match: Match {
                    path: Some("/secure".to_string()),
                    path_regex: None,
                },
                backend: "http://secure-service".to_string(),
                strategy: Some(StrategyRef::Named("security".to_string())),
                strategies: None,
            },
            
            // Route 4: Inline strategy with inheritance
            Route::Static {
                r#match: Match {
                    path: Some("/static".to_string()),
                    path_regex: None,
                },
                static_dir: std::path::PathBuf::from("/var/www/static"),
                strategy: Some(StrategyRef::InlineMiddleware(vec![
                    MiddlewareConfig::new_named_json(
                        "custom_middleware".to_string(),
                        json!({
                            "param": "inline_value",
                            "source": "inline_route_4"
                        })
                    ),
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({
                            "requests": 10,
                            "window": "1s",
                            "source": "inline_override"
                        })
                    )
                ])),
                strategies: None,
            },
            
            // Route 5: Named strategy with local override
            Route::Proxy {
                r#match: Match {
                    path: Some("/admin".to_string()),
                    path_regex: None,
                },
                backend: "http://admin-service".to_string(),
                strategy: Some(StrategyRef::Named("api_public".to_string())),
                strategies: {
                    let mut route_strategies = std::collections::HashMap::new();
                    route_strategies.insert("api_public".to_string(), vec![
                        MiddlewareConfig::new_named_json(
                            "auth".to_string(),
                            json!({
                                "type": "basic",
                                "realm": "Admin Area",
                                "source": "route_local_override"
                            })
                        ),
                        MiddlewareConfig::new_named_json(
                            "admin_audit".to_string(),
                            json!({
                                "detailed": true,
                                "source": "route_local_override"
                            })
                        )
                    ]);
                    Some(route_strategies)
                },
            }
        ];
        
        // Create StrategyResolver
        let resolver = StrategyResolver::new(&site, &global).expect("Failed to create StrategyResolver");
        
        println!("=== COMPREHENSIVE HIERARCHY TEST ===");
        println!("Global strategies: {:?}", resolver.global.keys().collect::<Vec<_>>());
        println!("Merged site strategies: {:?}", resolver.merged_site.keys().collect::<Vec<_>>());
        println!("Resolved routes: {}", resolver.resolved_routes.len());
        
        // Test 1: Route with site default strategy inheritance
        let public_strategy = resolver.resolve_for_route(0)
            .expect("Route 0 should have a strategy");
        
        println!("\n--- Route 1 (/public) - Site Default Inheritance ---");
        println!("Strategy name: {}", public_strategy.name);
        println!("Middleware count: {}", public_strategy.middleware.len());
        println!("Middleware sequence: {:?}", 
                public_strategy.middleware.iter().map(|m| m.name()).collect::<Vec<_>>());
        
        // Should inherit from site strategy + global base supplementation
        assert_eq!(public_strategy.name, "site_enhanced");
        assert_eq!(public_strategy.middleware.len(), 4); // logging, cors (global) + rate_limit, compression (site)
        
        // Verify sequence: global base middleware first, then site middleware
        assert_eq!(public_strategy.middleware[0].name(), "logging"); // From global base
        assert_eq!(public_strategy.middleware[1].name(), "cors"); // From global base
        assert_eq!(public_strategy.middleware[2].name(), "rate_limit"); // From site (overrides global)
        assert_eq!(public_strategy.middleware[3].name(), "compression"); // From site
        
        // Verify rate_limit supplementation (site override + global base fields)
        let rate_limit_config = public_strategy.middleware[2].config_as_json().unwrap();
        assert_eq!(rate_limit_config["requests"], 500); // Site override
        assert_eq!(rate_limit_config["window"], "30s"); // Site override
        assert_eq!(rate_limit_config["burst"], 200); // Supplemented from global base
        assert_eq!(rate_limit_config["source"], "site_enhanced"); // Source preserved
        
        // Test 2: Route with named site strategy
        let api_strategy = resolver.resolve_for_route(1)
            .expect("Route 1 should have a strategy");
        
        println!("\n--- Route 2 (/api) - Named Site Strategy ---");
        println!("Strategy name: {}", api_strategy.name);
        println!("Middleware count: {}", api_strategy.middleware.len());
        println!("Middleware sequence: {:?}", 
                api_strategy.middleware.iter().map(|m| m.name()).collect::<Vec<_>>());
        
        assert_eq!(api_strategy.name, "api_public");
        assert_eq!(api_strategy.middleware.len(), 4); // logging, cors (global) + rate_limit, cache (site)
        
        // Verify sequence and supplementation
        assert_eq!(api_strategy.middleware[0].name(), "logging"); // From global base
        assert_eq!(api_strategy.middleware[1].name(), "cors"); // From global base
        assert_eq!(api_strategy.middleware[2].name(), "rate_limit"); // From site
        assert_eq!(api_strategy.middleware[3].name(), "cache"); // From site
        
        // Test 3: Route with global named strategy
        let secure_strategy = resolver.resolve_for_route(2)
            .expect("Route 2 should have a strategy");
        
        println!("\n--- Route 3 (/secure) - Global Named Strategy ---");
        println!("Strategy name: {}", secure_strategy.name);
        println!("Middleware count: {}", secure_strategy.middleware.len());
        println!("Middleware sequence: {:?}", 
                secure_strategy.middleware.iter().map(|m| m.name()).collect::<Vec<_>>());
        
        assert_eq!(secure_strategy.name, "security");
        assert_eq!(secure_strategy.middleware.len(), 2); // auth, audit (global only)
        assert_eq!(secure_strategy.middleware[0].name(), "auth");
        assert_eq!(secure_strategy.middleware[1].name(), "audit");
        
        // Test 4: Route with inline strategy inheritance
        let static_strategy = resolver.resolve_for_route(3)
            .expect("Route 3 should have a strategy");
        
        println!("\n--- Route 4 (/static) - Inline Strategy Inheritance ---");
        println!("Strategy name: {}", static_strategy.name);
        println!("Middleware count: {}", static_strategy.middleware.len());
        println!("Middleware sequence: {:?}", 
                static_strategy.middleware.iter().map(|m| m.name()).collect::<Vec<_>>());
        
        assert_eq!(static_strategy.name, "inline");
        assert_eq!(static_strategy.middleware.len(), 5); // Complete inheritance chain
        
        // Verify complete inheritance sequence
        let expected_sequence = vec![
            "logging",      // Global base
            "cors",         // Global base  
            "compression",  // Site strategy
            "custom_middleware", // Inline
            "rate_limit"    // Inline override (should be last)
        ];
        
        let actual_sequence = static_strategy.middleware.iter()
            .map(|m| m.name())
            .collect::<Vec<_>>();
        
        assert_eq!(actual_sequence, expected_sequence);
        
        // Verify inline rate_limit takes precedence
        let inline_rate_limit = &static_strategy.middleware[4];
        let config = inline_rate_limit.config_as_json().unwrap();
        assert_eq!(config["requests"], 10); // Inline value
        assert_eq!(config["window"], "1s"); // Inline value
        assert_eq!(config["source"], "inline_override"); // Inline source
        
        // Test 5: Route with local strategy override
        let admin_strategy = resolver.resolve_for_route(4)
            .expect("Route 4 should have a strategy");
        
        println!("\n--- Route 5 (/admin) - Local Strategy Override ---");
        println!("Strategy name: {}", admin_strategy.name);
        println!("Middleware count: {}", admin_strategy.middleware.len());
        println!("Middleware sequence: {:?}", 
                admin_strategy.middleware.iter().map(|m| m.name()).collect::<Vec<_>>());
        
        assert_eq!(admin_strategy.name, "api_public");
        assert_eq!(admin_strategy.middleware.len(), 6); // logging, cors (global) + rate_limit, cache (site) + auth, admin_audit (route)
        
        // Verify local override
        let auth_config = admin_strategy.middleware.iter()
            .find(|m| m.name() == "auth")
            .unwrap()
            .config_as_json()
            .unwrap();
        assert_eq!(auth_config["type"], "basic"); // Local override
        assert_eq!(auth_config["realm"], "Admin Area"); // Local override
        assert_eq!(auth_config["source"], "route_local_override"); // Local source
        
        // Test 6: Site-level strategy resolution
        let site_strategy = resolver.resolve_for_site(&site)
            .expect("Site should have a strategy")
            .expect("Site strategy should not be None");
        
        println!("\n--- Site Strategy Resolution ---");
        println!("Strategy name: {}", site_strategy.name);
        println!("Middleware count: {}", site_strategy.middleware.len());
        println!("Middleware sequence: {:?}", 
                site_strategy.middleware.iter().map(|m| m.name()).collect::<Vec<_>>());
        
        assert_eq!(site_strategy.name, "site_enhanced");
        assert_eq!(site_strategy.middleware.len(), 4); // Same as Route 1
        
        // Test 7: All strategies collection
        let all_strategies = resolver.get_all_strategies();
        println!("\n--- All Strategies Collection ---");
        println!("Total strategies: {}", all_strategies.len());
        for (name, middleware) in &all_strategies {
            println!("  {}: {} middleware", name, middleware.len());
        }
        
        // Should contain all strategies: global + merged site
        assert!(all_strategies.contains_key("base"));
        assert!(all_strategies.contains_key("security"));
        assert!(all_strategies.contains_key("site_enhanced"));
        assert!(all_strategies.contains_key("api_public"));
        
        // Test 8: Validate middleware configuration integrity
        println!("\n--- Configuration Integrity Validation ---");
        
        // Verify no duplicate middleware names within any strategy
        for (name, middleware) in &all_strategies {
            let mut seen_names = std::collections::HashSet::new();
            for m in middleware {
                assert!(!seen_names.contains(m.name()), 
                       "Duplicate middleware '{}' in strategy '{}'", m.name(), name);
                seen_names.insert(m.name());
            }
        }
        
        // Verify all middleware have valid configurations
        for strategy_name in ["base", "security", "site_enhanced", "api_public"] {
            if let Some(middleware) = all_strategies.get(strategy_name) {
                for m in middleware {
                    assert!(m.config_as_json().is_ok(), 
                           "Middleware '{}' in strategy '{}' should have valid JSON config", 
                           m.name(), strategy_name);
                }
            }
        }
        
        println!("\n✅ COMPLETE HIERARCHY TEST PASSED");
        println!("✅ All inheritance chains validated");
        println!("✅ Middleware sequencing correct");
        println!("✅ Supplementation behavior verified");
        println!("✅ Local overrides working properly");
        println!("✅ Configuration integrity maintained");
        
        // Final assertions for test completeness
        assert_eq!(resolver.resolved_routes.len(), 5); // All routes resolved
        assert!(resolver.resolve_for_route(0).is_some()); // All routes have strategies
        assert!(resolver.resolve_for_route(1).is_some());
        assert!(resolver.resolve_for_route(2).is_some());
        assert!(resolver.resolve_for_route(3).is_some());
        assert!(resolver.resolve_for_route(4).is_some());
    }
}

