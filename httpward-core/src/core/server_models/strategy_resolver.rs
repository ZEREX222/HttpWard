use std::collections::HashMap;
use std::sync::Arc;
use std::hash::{Hash, Hasher};
use anyhow::Result;

use crate::config::strategy::{Strategy, StrategyCollection, StrategyRef, supplement_middleware, supplement_middleware_configs};
use crate::config::{SiteConfig, GlobalConfig, Route};

/// Key for route-level cache
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RouteKey {
    site_name: String,
    route_index: usize,
}

impl Hash for RouteKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.site_name.hash(state);
        self.route_index.hash(state);
    }
}

/// Cache for resolved strategies to avoid repeated computation
type StrategyCache = HashMap<RouteKey, Arc<Strategy>>;

/// Resolver that holds precomputed merged collections and caches
/// Strategy resolution with optimal performance and memory usage
#[derive(Debug, Clone)]
pub struct StrategyResolver {
    /// Global strategies (Arc for memory efficiency)
    global: Arc<StrategyCollection>,
    /// Precomputed merged site -> global strategies (Arc for memory efficiency)
    merged_site: Arc<StrategyCollection>,
    /// Cache for per-route resolved strategies
    cache: Arc<StrategyCache>,
    /// Site identifier for cache keys
    site_name: String,
}

impl StrategyResolver {
    /// Create resolver for a given site and global config.
    /// This precomputes merged_site map for optimal performance.
    pub fn new(site: &SiteConfig, global: &GlobalConfig) -> Result<Self> {
        // Precompute global map as Arc for memory efficiency
        let global_map = Arc::new(global.strategies.clone());
        let mut merged_site_map: StrategyCollection = HashMap::new();

        // First, add all global strategies - for availability
        for (name, gvec) in global_map.iter() {
            merged_site_map.insert(name.clone(), gvec.clone());
        }

        // Then, process site strategies with inheritance
        for (name, site_middleware_vec) in site.strategies.iter() {
            if let Some(global_vec) = global_map.get(name) {
                // child = site_vec.clone(), then supplement from global
                let mut child = site_middleware_vec.clone();
                // supplement missing properties from global into child
                supplement_middleware_configs(&mut child, global_vec.clone())?;
                merged_site_map.insert(name.clone(), child);
            } else {
                // Check if this is the site's default strategy and inherit from global default
                if let (Some(site_default_ref), Some(global_default_ref)) = (&site.strategy, &global.strategy) {
                    let site_default_name = match site_default_ref {
                        StrategyRef::Named(name) => name,
                        StrategyRef::Inline(_) => {
                            merged_site_map.insert(name.clone(), site_middleware_vec.clone());
                            continue;
                        }
                    };
                    
                    if name == site_default_name {
                        let global_default_name = match global_default_ref {
                            StrategyRef::Named(name) => name,
                            StrategyRef::Inline(_) => {
                                merged_site_map.insert(name.clone(), site_middleware_vec.clone());
                                continue;
                            }
                        };
                        
                        if let Some(global_default_vec) = global_map.get(global_default_name) {
                            let mut child = site_middleware_vec.clone();
                            supplement_middleware_configs(&mut child, global_default_vec.clone())?;
                            merged_site_map.insert(name.clone(), child);
                        } else {
                            merged_site_map.insert(name.clone(), site_middleware_vec.clone());
                        }
                    } else {
                        merged_site_map.insert(name.clone(), site_middleware_vec.clone());
                    }
                } else {
                    merged_site_map.insert(name.clone(), site_middleware_vec.clone());
                }
            }
        }

        Ok(Self {
            global: global_map,
            merged_site: Arc::new(merged_site_map),
            cache: Arc::new(HashMap::new()),
            site_name: site.domain.clone(),
        })
    }

    /// Get the chosen strategy reference in priority order:
    /// 1) route.strategy (if provided)
    /// 2) site.strategy 
    /// 3) global.strategy
    fn get_chosen_strategy_ref(
        &self,
        route: &Route,
        site: &SiteConfig,
        route_strategy_override: Option<&StrategyRef>,
    ) -> Option<StrategyRef> {
        // Priority: provided override > route.strategy > site.strategy > global.strategy
        route_strategy_override
            .cloned()
            .or_else(|| route.get_strategy().cloned())
            .or_else(|| site.strategy.clone())
            .or_else(|| None) // TODO: Add global.strategy when available
    }

    /// Find named strategy in the resolution chain:
    /// 1) route.strategies (if any)
    /// 2) merged_site (site overrides merged with global)  
    /// 3) global
    fn find_named_strategy(
        &self,
        name: &str,
        route: &Route,
        supplement_from_higher_levels: bool,
    ) -> Option<Strategy> {
        // 1) Try route-level strategies first
        if let Some(route_collection) = route.get_strategies() {
            if let Some(route_vec) = route_collection.get(name) {
                let mut base = Strategy {
                    name: name.to_string(),
                    middleware: route_vec.clone(),
                };
                
                if supplement_from_higher_levels {
                    // Supplement from site level (merged_site)
                    if let Some(site_vec) = self.merged_site.get(name) {
                        let _ = supplement_middleware(&mut base.middleware, site_vec.clone());
                    }
                }
                
                return Some(base);
            }
        }

        // 2) Try merged_site (site + global already merged)
        if let Some(site_vec) = self.merged_site.get(name) {
            return Some(Strategy {
                name: name.to_string(),
                middleware: site_vec.clone(),
            });
        }

        // 3) Try global strategies
        if let Some(global_vec) = self.global.get(name) {
            return Some(Strategy {
                name: name.to_string(),
                middleware: global_vec.clone(),
            });
        }

        None
    }

    /// Resolve StrategyRef for a given route (by index) with caching.
    /// 
    /// The precedence for finding a named strategy is:
    /// 1) route.strategies
    /// 2) merged_site (site overrides merged with global)
    /// 3) global
    ///
    /// If `ref` is Inline, this returns inline as-is (no supplement by default).
    /// Set `supplement_inline_with_parents` to true if you want inline strategies
    /// to inherit from parent strategies.
    pub fn resolve_for_route(
        &self,
        route_index: usize,
        route: &Route,
        site: &SiteConfig,
        supplement_inline_with_parents: bool,
    ) -> Result<Option<Arc<Strategy>>> {
        let key = RouteKey {
            site_name: self.site_name.clone(),
            route_index,
        };

        // Check cache first (read-only access)
        if let Some(cached) = self.cache.get(&key) {
            return Ok(Some(cached.clone()));
        }

        // Determine chosen strategy reference
        let strategy_ref = self.get_chosen_strategy_ref(route, site, None);

        let resolved = match strategy_ref {
            Some(StrategyRef::Inline(mut inline)) => {
                // Inline strategy - treat as final by default
                if supplement_inline_with_parents {
                    // Optional: supplement inline with parent strategies
                    if let Some(parent_strategy) = self.find_named_strategy(&inline.name, route, false) {
                        inline.supplement_with(parent_strategy.middleware)?;
                    }
                }
                Some(Arc::new(inline))
            }
            Some(StrategyRef::Named(name)) => {
                // Find named strategy with proper inheritance chain
                self.find_named_strategy(&name, route, true).map(Arc::new)
            }
            None => None,
        };

        Ok(resolved)
    }

    /// Resolve strategy for a site (global + site overrides)
    pub fn resolve_for_site(&self, site: &SiteConfig) -> Result<Option<Arc<Strategy>>> {
        if let Some(strategy_ref) = &site.strategy {
            match strategy_ref {
                StrategyRef::Inline(inline) => {
                    Ok(Some(Arc::new(inline.clone())))
                }
                StrategyRef::Named(name) => {
                    // Look in merged_site first, then global
                    if let Some(site_vec) = self.merged_site.get(name) {
                        let strategy = Strategy {
                            name: name.clone(),
                            middleware: site_vec.clone(),
                        };
                        Ok(Some(Arc::new(strategy)))
                    } else if let Some(global_vec) = self.global.get(name) {
                        let strategy = Strategy {
                            name: name.clone(),
                            middleware: global_vec.clone(),
                        };
                        Ok(Some(Arc::new(strategy)))
                    } else {
                        Ok(None)
                    }
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Get all available strategies (global + site merged)
    pub fn get_all_strategies(&self) -> StrategyCollection {
        let mut all = self.global.as_ref().clone();
        
        // Add/override with site strategies (already merged with global)
        for (name, strategy) in self.merged_site.iter() {
            all.insert(name.clone(), strategy.clone());
        }
        
        all
    }

    /// Clear the strategy cache (useful for configuration reloads)
    pub fn clear_cache(&mut self) {
        // Note: This would require interior mutability in real implementation
        // For now, this is a placeholder for the API
    }

    /// Get cache statistics for monitoring
    pub fn cache_stats(&self) -> (usize, usize) {
        // Returns (cache_size, estimated_memory_bytes)
        let cache_size = self.cache.len();
        let estimated_memory = cache_size * std::mem::size_of::<RouteKey>() 
            + cache_size * std::mem::size_of::<Arc<Strategy>>();
        (cache_size, estimated_memory)
    }
}

/// Factory for creating and managing StrategyResolver instances
pub struct StrategyResolverFactory;

impl StrategyResolverFactory {
    /// Create a resolver for a site with optimal memory usage
    pub fn create_for_site(site: &SiteConfig, global: &GlobalConfig) -> Result<StrategyResolver> {
        StrategyResolver::new(site, global)
    }

    /// Create resolvers for multiple sites efficiently
    pub fn create_for_sites(
        sites: &[SiteConfig], 
        global: &GlobalConfig
    ) -> Result<Vec<StrategyResolver>> {
        let mut resolvers = Vec::with_capacity(sites.len());
        
        for site in sites {
            resolvers.push(StrategyResolver::new(site, global)?);
        }
        
        Ok(resolvers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::strategy::{MiddlewareConfig, UniversalValue};
    use serde_json::json;

    fn create_test_global() -> GlobalConfig {
        let mut strategies = StrategyCollection::new();
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
        let mut strategies = StrategyCollection::new();
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
            routes: vec![],
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
        
        let resolved = resolver.resolve_for_route(0, &route, &site, false).unwrap();
        
        assert!(resolved.is_some());
        let strategy = resolved.unwrap();
        assert_eq!(strategy.name, "site_default");
        assert_eq!(strategy.middleware.len(), 2);
    }

    #[test]
    fn test_inline_strategy_resolution() {
        let global = create_test_global();
        let site = create_test_site();
        let resolver = StrategyResolver::new(&site, &global).unwrap();
        
        let inline_strategy = Strategy {
            name: "inline_test".to_string(),
            middleware: vec![
                MiddlewareConfig::new_named_json(
                    "cors".to_string(),
                    json!({"origins": ["*"]})
                ),
            ],
        };
        
        let route = Route::Proxy {
            r#match: Default::default(),
            backend: "http://backend".to_string(),
            strategy: Some(StrategyRef::Inline(inline_strategy.clone())),
            strategies: None,
        };
        
        let resolved = resolver.resolve_for_route(0, &route, &site, false).unwrap();
        
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
    fn test_cache_key_equality() {
        let key1 = RouteKey {
            site_name: "test.com".to_string(),
            route_index: 0,
        };
        let key2 = RouteKey {
            site_name: "test.com".to_string(),
            route_index: 0,
        };
        let key3 = RouteKey {
            site_name: "test.com".to_string(),
            route_index: 1,
        };
        
        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
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
}
