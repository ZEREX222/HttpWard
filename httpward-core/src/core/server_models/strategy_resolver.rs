use std::collections::HashMap;
use std::sync::Arc;
use std::hash::{Hash, Hasher};
use anyhow::Result;
use tracing::{debug, trace, instrument};

use crate::config::strategy::{Strategy, StrategyRef, supplement_middleware_configs, supplement_middleware, LegacyStrategyCollection};
use crate::config::{SiteConfig, GlobalConfig, Route, MiddlewareConfig};

/// Optimized strategy collection using Arc to reduce cloning
pub type StrategyCollection = HashMap<String, Arc<Vec<MiddlewareConfig>>>;

/// High-performance resolver with precomputed strategies and no runtime overhead
#[derive(Debug, Clone)]
pub struct StrategyResolver {
    /// Global strategies (Arc for memory efficiency)
    global: Arc<StrategyCollection>,
    /// Precomputed merged site -> global strategies (Arc for memory efficiency)
    merged_site: Arc<StrategyCollection>,
    /// Precomputed resolved strategies for all routes (eliminates runtime resolution)
    resolved_routes: Vec<Option<Arc<Strategy>>>,
}

impl StrategyResolver {
    /// Create resolver for a given site and global config.
    /// This precomputes merged_site map and resolves all routes for optimal performance.
    #[instrument(skip(site, global))]
    pub fn new(site: &SiteConfig, global: &GlobalConfig) -> Result<Self> {
        debug!("Creating StrategyResolver for site: {}", site.domain);
        
        // Convert legacy collections to optimized Arc-based collections
        let global_map: StrategyCollection = global.strategies
            .iter()
            .map(|(name, vec)| (name.clone(), Arc::new(vec.clone())))
            .collect();
        let global_map = Arc::new(global_map);
        
        // Simplified merged_site creation: start with global, then overlay site
        let mut merged_site_map: StrategyCollection = global_map.iter()
            .map(|(name, arc_vec): (&String, &Arc<Vec<MiddlewareConfig>>)| (name.clone(), arc_vec.clone()))
            .collect();
        
        // Process site strategies with inheritance
        for (name, site_middleware_vec) in site.strategies.iter() {
            let site_arc = Arc::new(site_middleware_vec.clone());
            
            if let Some(global_arc) = global_map.get(name) {
                // Direct name match - merge site overrides global
                let mut merged = site_middleware_vec.clone();
                supplement_middleware_configs(&mut merged, (**global_arc).clone())?;
                merged_site_map.insert(name.clone(), Arc::new(merged));
            } else {
                // Check if this is the site's default strategy and inherit from global default
                if let (Some(site_default_ref), Some(global_default_ref)) = (&site.strategy, &global.strategy) {
                    let site_default_name = match site_default_ref {
                        StrategyRef::Named(name) => name,
                        StrategyRef::Inline(_) => {
                            merged_site_map.insert(name.clone(), site_arc);
                            continue;
                        }
                    };
                    
                    if name == site_default_name {
                        let global_default_name = match global_default_ref {
                            StrategyRef::Named(name) => name,
                            StrategyRef::Inline(_) => {
                                merged_site_map.insert(name.clone(), site_arc);
                                continue;
                            }
                        };
                        
                        if let Some(global_default_arc) = global_map.get(global_default_name) {
                            let mut merged = site_middleware_vec.clone();
                            supplement_middleware_configs(&mut merged, (**global_default_arc).clone())?;
                            merged_site_map.insert(name.clone(), Arc::new(merged));
                        } else {
                            merged_site_map.insert(name.clone(), site_arc);
                        }
                    } else {
                        merged_site_map.insert(name.clone(), site_arc);
                    }
                } else {
                    merged_site_map.insert(name.clone(), site_arc);
                }
            }
        }
        
        let merged_site = Arc::new(merged_site_map);
        
        // Precompute resolved strategies for all routes
        let mut resolved_routes = Vec::with_capacity(site.routes.len());
        for (index, route) in site.routes.iter().enumerate() {
            let resolved = Self::resolve_single_route_static(route, site, &merged_site, false, &global.strategy)?;
            resolved_routes.push(resolved);
            trace!("Precomputed strategy for route {}: {:?}", index, resolved_routes[index].as_ref().map(|s| &s.name));
        }
        
        Ok(Self {
            global: global_map,
            merged_site,
            resolved_routes,
        })
    }

    /// Static helper for getting chosen strategy reference
    fn get_chosen_strategy_ref_static(
        route: &Route,
        site: &SiteConfig,
        route_strategy_override: Option<&StrategyRef>,
        global_default: &Option<StrategyRef>,
    ) -> Option<StrategyRef> {
        // Priority: provided override > route.strategy > site.strategy > global.strategy
        route_strategy_override
            .cloned()
            .or_else(|| route.get_strategy().cloned())
            .or_else(|| site.strategy.clone())
            .or_else(|| global_default.clone())
    }

    /// Static helper for finding named strategy
    fn find_named_strategy_static(
        name: &str,
        route: &Route,
        merged_site: &StrategyCollection,
        supplement_from_higher_levels: bool,
    ) -> Option<Strategy> {
        // 1) Try route-level strategies first
        if let Some(route_collection) = route.get_strategies() {
            if let Some(route_vec) = route_collection.get(name) {
                let mut base = Strategy {
                    name: name.to_string(),
                    middleware: Arc::new(route_vec.clone()),
                };
                
                if supplement_from_higher_levels {
                    // Supplement from site level (merged_site)
                    if let Some(site_arc) = merged_site.get(name) {
                        let _ = supplement_middleware(Arc::make_mut(&mut base.middleware), (**site_arc).clone());
                    }
                }
                
                return Some(base);
            }
        }

        // 2) Try merged_site (site + global already merged)
        if let Some(site_arc) = merged_site.get(name) {
            return Some(Strategy {
                name: name.to_string(),
                middleware: site_arc.clone(),
            });
        }

        None
    }

    /// Helper method to resolve a single route strategy (used during precomputation)
    fn resolve_single_route_static(
        route: &Route,
        site: &SiteConfig,
        merged_site: &StrategyCollection,
        supplement_inline_with_parents: bool,
        global_default: &Option<StrategyRef>,
    ) -> Result<Option<Arc<Strategy>>> {
        // Determine chosen strategy reference
        let strategy_ref = Self::get_chosen_strategy_ref_static(route, site, None, global_default);

        let resolved = match strategy_ref {
            Some(StrategyRef::Inline(mut inline)) => {
                // Inline strategy - treat as final by default
                if supplement_inline_with_parents {
                    // Optional: supplement inline with parent strategies
                    if let Some(parent_strategy) = Self::find_named_strategy_static(&inline.name, route, merged_site, false) {
                        inline.supplement_with(parent_strategy.middleware.as_ref().clone())?;
                    }
                }
                Some(Arc::new(inline))
            }
            Some(StrategyRef::Named(name)) => {
                // Find named strategy with proper inheritance chain
                Self::find_named_strategy_static(&name, route, merged_site, true).map(Arc::new)
            }
            None => None,
        };

        Ok(resolved)
    }

    /// Resolve strategy for a given route (by index) - ultra-fast lookup
    /// Uses precomputed routes only - no runtime overhead
    #[instrument(skip(self))]
    pub fn resolve_for_route(&self, route_index: usize) -> Option<Arc<Strategy>> {
        if route_index >= self.resolved_routes.len() {
            return None;
        }
        self.resolved_routes.get(route_index).cloned().flatten()
    }

    /// Resolve strategy for a site (global + site overrides)
    #[instrument(skip(self, site))]
    pub fn resolve_for_site(&self, site: &SiteConfig) -> Result<Option<Arc<Strategy>>> {
        if let Some(strategy_ref) = &site.strategy {
            match strategy_ref {
                StrategyRef::Inline(inline) => {
                    debug!("Resolving inline site strategy: {}", inline.name);
                    Ok(Some(Arc::new(inline.clone())))
                }
                StrategyRef::Named(name) => {
                    debug!("Resolving named site strategy: {}", name);
                    // Look in merged_site first (already contains global + site)
                    if let Some(site_arc) = self.merged_site.get(name) {
                        let strategy = Strategy {
                            name: name.clone(),
                            middleware: site_arc.clone(),
                        };
                        Ok(Some(Arc::new(strategy)))
                    } else {
                        trace!("Site strategy '{}' not found in merged_site", name);
                        Ok(None)
                    }
                }
            }
        } else {
            trace!("No site strategy defined");
            Ok(None)
        }
    }

    /// Get all available strategies (global + site merged)
    pub fn get_all_strategies(&self) -> LegacyStrategyCollection {
        let mut all = self.global.iter()
            .map(|(name, arc_vec): (&String, &Arc<Vec<MiddlewareConfig>>)| (name.clone(), (**arc_vec).clone()))
            .collect::<LegacyStrategyCollection>();
        
        // Add/override with site strategies (already merged with global)
        for (name, arc_vec) in self.merged_site.iter() {
            all.insert(name.clone(), (**arc_vec).clone());
        }
        
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::strategy::{MiddlewareConfig, UniversalValue};
    use serde_json::json;

    fn create_test_global() -> GlobalConfig {
        let mut strategies = LegacyStrategyCollection::new();
        strategies.insert(
            "default".to_string(),
            vec![
                MiddlewareConfig::new_named_json(
                    "rate_limit".to_string(),
                    json!({"requests": 1000, "window": "1m"})
                ),
                MiddlewareConfig::new_named_json(
                    "logging".to_string(),
                    json!({"level": "info"})
                ),
            ]
        );
        
        GlobalConfig {
            domain: "example.com".to_string(),
            domains: vec![],
            listeners: vec![],
            routes: vec![],
            log: Default::default(),
            sites_enabled: Default::default(),
            strategy: Some(StrategyRef::Named("default".to_string())),
            strategies,
        }
    }

    fn create_test_site() -> SiteConfig {
        let mut strategies = LegacyStrategyCollection::new();
        strategies.insert(
            "site_default".to_string(),
            vec![
                MiddlewareConfig::new_named_json(
                    "rate_limit".to_string(),
                    json!({"requests": 500}) // Missing "window" - should inherit from global
                ),
                MiddlewareConfig::new_named_json(
                    "auth".to_string(),
                    json!({"type": "jwt"})
                ),
            ]
        );

        SiteConfig {
            domain: "test.example.com".to_string(),
            domains: vec![],
            listeners: vec![],
            routes: vec![
                Route::Proxy {
                    r#match: Default::default(),
                    backend: "http://backend".to_string(),
                    strategy: Some(StrategyRef::Named("site_default".to_string())),
                    strategies: None,
                }
            ],
            strategy: Some(StrategyRef::Named("site_default".to_string())),
            strategies,
        }
    }

    #[test]
    fn test_strategy_resolver_creation() {
        let global = create_test_global();
        let site = create_test_site();
        
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        // Check that global strategies are available
        assert!(resolver.global.contains_key("default"));
        
        // Check that site strategies are merged
        assert!(resolver.merged_site.contains_key("site_default"));
        assert!(resolver.merged_site.contains_key("default")); // Should inherit global
        
        // Check site strategy supplementation
        let site_default = &resolver.merged_site["site_default"];
        assert_eq!(site_default.len(), 2); // rate_limit + auth
        
        // Check rate_limit inherited "window" from global
        let rate_limit = &site_default[0];
        assert_eq!(rate_limit.name(), "rate_limit");
        let config = rate_limit.config_as_json().unwrap();
        assert_eq!(config["requests"], 500); // Site value
        assert_eq!(config["window"], "1m"); // Inherited from global
        
        // Check that resolved_routes is precomputed
        assert_eq!(resolver.resolved_routes.len(), site.routes.len());
    }

    #[test]
    fn test_named_strategy_resolution() {
        let global = create_test_global();
        let site = create_test_site();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        // Create a test route with strategy reference
        let route = Route::Proxy {
            r#match: Default::default(),
            backend: "http://backend".to_string(),
            strategy: Some(StrategyRef::Named("site_default".to_string())),
            strategies: None,
        };
        
        let resolved = resolver.resolve_for_route(0);
        
        assert!(resolved.is_some());
        let strategy = resolved.unwrap();
        assert_eq!(strategy.name, "site_default");
        assert_eq!(strategy.middleware.len(), 2);
    }

    #[test]
    fn test_precomputed_routes_only() {
        let global = create_test_global();
        let site = create_test_site();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        // Test that all routes are precomputed
        assert_eq!(resolver.resolved_routes.len(), site.routes.len());
        
        // Test that resolve_for_route works with precomputed routes only
        for (index, _route) in site.routes.iter().enumerate() {
            let resolved = resolver.resolve_for_route(index);
            assert!(resolved.is_some(), "Route {} should have precomputed strategy", index);
        }
        
        // Test that invalid index returns None (no fallback computation)
        let resolved = resolver.resolve_for_route(999);
        assert!(resolved.is_none(), "Invalid route index should return None");
        
        // Test that valid index works
        let resolved = resolver.resolve_for_route(0);
        assert!(resolved.is_some(), "Valid route index should return strategy");
    }

    #[test]
    fn test_global_strategy_inheritance() {
        let global = create_test_global();
        let site = create_test_site();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        // Create a site without strategy (should inherit from global)
        let mut site_no_strategy = site.clone();
        site_no_strategy.strategy = None;
        // Update the route to not have a strategy either
        site_no_strategy.routes[0] = Route::Proxy {
            r#match: Default::default(),
            backend: "http://backend".to_string(),
            strategy: None,
            strategies: None,
        };
        
        // Create a new resolver with the modified site
        let resolver_no_strategy = StrategyResolver::new(&site_no_strategy, &global).unwrap();
        
        let resolved = resolver_no_strategy.resolve_for_route(0);
        
        // Should inherit global.default strategy
        assert!(resolved.is_some());
        let strategy = resolved.unwrap();
        assert_eq!(strategy.name, "default");
        assert_eq!(strategy.middleware.len(), 2); // rate_limit + logging
    }

    #[test]
    fn test_inline_strategy_resolution() {
        let global = create_test_global();
        let site = create_test_site();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        let inline_strategy = Strategy {
            name: "inline_test".to_string(),
            middleware: Arc::new(vec![
                MiddlewareConfig::new_named_json(
                    "cors".to_string(),
                    json!({"origins": ["*"]})
                ),
            ])
        };
        
        // Create a site with inline strategy
        let mut site_inline = site.clone();
        site_inline.routes[0] = Route::Proxy {
            r#match: Default::default(),
            backend: "http://backend".to_string(),
            strategy: Some(StrategyRef::Inline(inline_strategy.clone())),
            strategies: None,
        };
        
        let resolver_inline = StrategyResolver::new(&site_inline, &global).unwrap();
        
        let resolved = resolver_inline.resolve_for_route(0);
        
        assert!(resolved.is_some());
        let strategy = resolved.unwrap();
        assert_eq!(strategy.name, "inline_test");
        assert_eq!(strategy.middleware.len(), 1);
        assert_eq!(strategy.middleware[0].name(), "cors");
    }

    #[test]
    fn test_strategy_inheritance_chain() {
        let global = create_test_global();
        let site = create_test_site();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        // Test that site strategies inherit from global when missing properties
        let site_default = &resolver.merged_site["site_default"];
        let rate_limit = &site_default[0];
        let config = rate_limit.config_as_json().unwrap();
        
        // Site has requests=500, should inherit window="1m" from global default
        assert_eq!(config["requests"], 500);
        assert_eq!(config["window"], "1m");
    }

    #[test]
    fn test_get_all_strategies() {
        let global = create_test_global();
        let site = create_test_site();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        let all = resolver.get_all_strategies();
        
        // Should contain both global and site strategies
        assert!(all.contains_key("default"));
        assert!(all.contains_key("site_default"));
        assert_eq!(all.len(), 2);
        
        // Verify the strategies are properly merged
        let site_default = &all["site_default"];
        assert_eq!(site_default.len(), 2);
        assert_eq!(site_default[0].name(), "rate_limit");
    }

    #[test]
    fn test_resolve_for_site() {
        let global = create_test_global();
        let site = create_test_site();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        let resolved = resolver.resolve_for_site(&site).unwrap();
        
        assert!(resolved.is_some());
        let strategy = resolved.unwrap();
        assert_eq!(strategy.name, "site_default");
        assert_eq!(strategy.middleware.len(), 2);
    }

    #[test]
    fn test_precomputed_routes_performance() {
        let global = create_test_global();
        let site = create_test_site();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        // Create a test route
        let route = Route::Proxy {
            r#match: Default::default(),
            backend: "http://backend".to_string(),
            strategy: Some(StrategyRef::Named("site_default".to_string())),
            strategies: None,
        };
        
        // Resolution for precomputed route index should be instant
        // Test site has 1 route, so index 0 should work
        let resolved = resolver.resolve_for_route(0);
        assert!(resolved.is_some()); // Should resolve precomputed strategy
    }
}
