use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::path::PathBuf;
use matchit::Router;
use regex::{Regex, RegexSet};
use thiserror::Error;
use crate::config::{SiteConfig, Route, GlobalConfig};
use crate::config::strategy::{MiddlewareConfig, Strategy, UniversalValue};
use super::strategy_resolver::StrategyResolver;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone)]
pub struct TlsPaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// Combined object containing a Route and its resolved active strategy
#[derive(Clone)]
pub struct RouteWithStrategy {
    /// The route definition
    pub route: Arc<Route>,
    /// The resolved active strategy for this route
    pub active_strategy: Arc<Strategy>,
    /// Fast O(1) lookup: middleware name -> index in active_strategy.middleware.
    middleware_index: Arc<HashMap<String, usize>>,
    /// Typed config cache: (middleware_index, TypeId) -> Arc<T> erased as Any.
    typed_cache: Arc<RwLock<HashMap<(usize, TypeId), Arc<dyn Any + Send + Sync>>>>,
}

impl std::fmt::Debug for RouteWithStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouteWithStrategy")
            .field("route", &self.route)
            .field("active_strategy", &self.active_strategy)
            .field("middleware_index_size", &self.middleware_index.len())
            .finish()
    }
}

impl RouteWithStrategy {
    pub fn new(route: Arc<Route>, active_strategy: Arc<Strategy>) -> Self {
        let mut middleware_index = HashMap::new();
        for (idx, mw) in active_strategy.middleware.iter().enumerate() {
            middleware_index.entry(mw.name().to_string()).or_insert(idx);
        }

        Self {
            route,
            active_strategy,
            middleware_index: Arc::new(middleware_index),
            typed_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Fast middleware config lookup by middleware name.
    pub fn middleware_config(&self, middleware_name: &str) -> Option<&MiddlewareConfig> {
        let idx = *self.middleware_index.get(middleware_name)?;
        self.active_strategy.middleware.get(idx)
    }

    /// Deserialize and cache typed middleware config for this route.
    ///
    /// Returns `Ok(None)` when middleware is not present or explicitly `Off`.
    pub fn middleware_config_typed<T>(&self, middleware_name: &str) -> anyhow::Result<Option<Arc<T>>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let idx = match self.middleware_index.get(middleware_name) {
            Some(idx) => *idx,
            None => return Ok(None),
        };

        if matches!(self.active_strategy.middleware[idx], MiddlewareConfig::Off { .. }) {
            return Ok(None);
        }

        let key = (idx, TypeId::of::<T>());

        if let Some(cached) = self
            .typed_cache
            .read()
            .map_err(|_| anyhow::anyhow!("typed config cache lock poisoned"))?
            .get(&key)
            .cloned()
        {
            if let Some(typed) = cached.downcast_ref::<Arc<T>>() {
                return Ok(Some(typed.clone()));
            }
        }

        let parsed = Arc::new(parse_middleware_config_typed::<T>(&self.active_strategy.middleware[idx])?);

        self.typed_cache
            .write()
            .map_err(|_| anyhow::anyhow!("typed config cache lock poisoned"))?
            .insert(key, Arc::new(parsed.clone()) as Arc<dyn Any + Send + Sync>);

        Ok(Some(parsed))
    }
}

fn parse_middleware_config_typed<T>(cfg: &MiddlewareConfig) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    match cfg {
        MiddlewareConfig::Named { config, .. } => match config {
            UniversalValue::Json(v) => serde_json::from_value(v.clone())
                .map_err(|e| anyhow::anyhow!("Failed to parse middleware JSON config: {}", e)),
            UniversalValue::Yaml(v) => serde_yaml::from_value(v.clone())
                .map_err(|e| anyhow::anyhow!("Failed to parse middleware YAML config: {}", e)),
        },
        MiddlewareConfig::On { .. } => serde_json::from_value(serde_json::Value::Object(serde_json::Map::new()))
            .map_err(|e| anyhow::anyhow!("Failed to parse default middleware config: {}", e)),
        MiddlewareConfig::Off { .. } => Err(anyhow::anyhow!("Middleware is disabled")),
    }
}

/// A mapping between a set of domains and their specific certificate files.
/// Used for SNI (Server Name Indication) lookup during the TLS handshake.
#[derive(Debug, Clone)]
pub struct TlsMapping {
    pub domains: Vec<String>,
    pub paths: TlsPaths,
}

#[derive(Error, Debug)]
pub enum SiteManagerError {
    #[error("invalid regex pattern: {0}")]
    InvalidRegex(String),
    #[error("invalid path pattern: {0}")]
    InvalidPath(String),
    #[error("no route matched")]
    NoMatch,
}

impl From<anyhow::Error> for SiteManagerError {
    fn from(err: anyhow::Error) -> Self {
        SiteManagerError::InvalidRegex(err.to_string())
    }
}

/// Matched route with parameters and active strategy
#[derive(Debug, Clone)]
pub struct MatchedRoute {
    pub route: Arc<Route>,
    pub active_strategy: Arc<Strategy>,
    pub params: HashMap<String, String>,
    pub matcher_type: MatcherType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatcherType {
    Path,
    Regex,
}

/// Site manager that handles route matching for a specific site
#[derive(Debug, Clone)]
pub struct SiteManager {
    site_config: Arc<SiteConfig>,
    /// matchit router for path patterns
    path_router: Router<usize>,
    /// regex patterns with route indices for captures
    regex_list: Vec<(Regex, usize)>,
    /// RegexSet for fast bulk matching
    regex_set: Option<RegexSet>,
    /// routes with their resolved strategies
    routes_with_strategy: Vec<RouteWithStrategy>,
    /// TLS mappings for this site's domains
    tls_mappings: Vec<TlsMapping>,
}

impl SiteManager {
    /// Create a new site manager with global config for strategy resolution
    pub fn new(site_config: Arc<SiteConfig>, global_config: Option<&GlobalConfig>) -> Result<Self, SiteManagerError> {
        let routes = site_config.routes.clone();
        let mut path_router = Router::new();
        let mut regex_raw: Vec<(String, usize)> = Vec::new();
        let mut routes_arc: Vec<Arc<Route>> = Vec::new();

        // Process routes and build matchers
        for (index, route) in routes.iter().enumerate() {
            let match_config = match route {
                Route::Proxy { r#match, .. } => r#match,
                Route::Static { r#match, .. } => r#match,
                Route::Redirect { r#match, .. } => r#match,
            };

            // Add path pattern to matchit router if present
            if let Some(path) = &match_config.path {
                path_router
                    .insert(path.clone(), index)
                    .map_err(|e| SiteManagerError::InvalidPath(format!("{}: {}", path, e)))?;
            }

            // Store raw regex patterns for later RegexSet compilation
            if let Some(path_regex) = &match_config.path_regex {
                regex_raw.push((path_regex.clone(), index));
            }

            routes_arc.push(Arc::new(route.clone()));
        }

        // Compile RegexSet and individual regexes
        let mut regex_list: Vec<(Regex, usize)> = Vec::new();
        let regex_set = if !regex_raw.is_empty() {
            let patterns: Vec<String> = regex_raw.iter().map(|(p, _)| p.clone()).collect();
            let set = RegexSet::new(&patterns)
                .map_err(|e| SiteManagerError::InvalidRegex(format!("RegexSet: {}", e)))?;
            
            // Compile individual regexes for captures
            for (pat, idx) in regex_raw.into_iter() {
                let r = Regex::new(&pat)
                    .map_err(|e| SiteManagerError::InvalidRegex(format!("{}: {}", pat, e)))?;
                regex_list.push((r, idx));
            }
            Some(set)
        } else {
            None
        };

        // Create strategy resolver if global config is provided
        let strategy_resolver = if let Some(global) = global_config {
            Some(Arc::new(StrategyResolver::new(&site_config, global)?))
        } else {
            None
        };

        // Create RouteWithStrategy objects
        let mut routes_with_strategy = Vec::new();
        if let Some(resolver) = &strategy_resolver {
            for (index, route) in routes_arc.iter().enumerate() {
                if let Some(strategy) = resolver.resolve_for_route(index) {
                    routes_with_strategy.push(RouteWithStrategy::new(route.clone(), strategy));
                }
            }
        }

        Ok(Self {
            site_config,
            path_router,
            regex_list,
            regex_set,
            routes_with_strategy,
            tls_mappings: Vec::new()
        })
    }

    /// Get reference to the site configuration
    pub fn site_config(&self) -> &Arc<SiteConfig> {
        &self.site_config
    }

    /// Get route for the given URL path with optimal performance
    pub fn get_route(&self, path: &str) -> Result<MatchedRoute, SiteManagerError> {
        // First try matchit path patterns (fastest)
        if let Ok(matched) = self.path_router.at(path) {
            let route_index = *matched.value;
            let route_with_strategy = &self.routes_with_strategy[route_index];
            let mut params = HashMap::with_capacity(matched.params.len());
            for (k, v) in matched.params.iter() {
                params.insert(k.to_string(), v.to_string());
            }

            return Ok(MatchedRoute {
                route: route_with_strategy.route.clone(),
                active_strategy: route_with_strategy.active_strategy.clone(),
                params,
                matcher_type: MatcherType::Path,
            });
        }

        // Use RegexSet for fast bulk matching
        if let Some(set) = &self.regex_set {
            let matches = set.matches(path);
            if matches.matched_any() {
                // Process only matched patterns in order
                for pat_idx in matches.iter() {
                    let (regex, route_index) = &self.regex_list[pat_idx];
                    if let Some(caps) = regex.captures(path) {
                        let mut params = HashMap::new();
                        
                        // Try named capture groups first
                        let mut has_named = false;
                        for name in regex.capture_names().flatten() {
                            if let Some(m) = caps.name(name) {
                                params.insert(name.to_string(), m.as_str().to_string());
                                has_named = true;
                            }
                        }
                        
                        // Fallback to numeric groups if no named groups
                        if !has_named {
                            for (i, m) in caps.iter().enumerate().skip(1) {
                                if let Some(m) = m {
                                    params.insert(i.to_string(), m.as_str().to_string());
                                }
                            }
                        }

                        let route_with_strategy = &self.routes_with_strategy[*route_index];
                        return Ok(MatchedRoute {
                            route: route_with_strategy.route.clone(),
                            active_strategy: route_with_strategy.active_strategy.clone(),
                            params,
                            matcher_type: MatcherType::Regex,
                        });
                    }
                }
            }
        } else {
            // Fallback to sequential regex checking (compatibility)
            for (regex, route_index) in &self.regex_list {
                if let Some(caps) = regex.captures(path) {
                    let mut params = HashMap::new();
                    
                    // Try named capture groups first
                    let mut has_named = false;
                    for name in regex.capture_names().flatten() {
                        if let Some(m) = caps.name(name) {
                            params.insert(name.to_string(), m.as_str().to_string());
                            has_named = true;
                        }
                    }
                    
                    // Fallback to numeric groups if no named groups
                    if !has_named {
                        for (i, m) in caps.iter().enumerate().skip(1) {
                            if let Some(m) = m {
                                params.insert(i.to_string(), m.as_str().to_string());
                            }
                        }
                    }

                    let route_with_strategy = &self.routes_with_strategy[*route_index];
                    return Ok(MatchedRoute {
                        route: route_with_strategy.route.clone(),
                        active_strategy: route_with_strategy.active_strategy.clone(),
                        params,
                        matcher_type: MatcherType::Regex,
                    });
                }
            }
        }

        Err(SiteManagerError::NoMatch)
    }

    /// Get all routes (for debugging)
    pub fn routes(&self) -> Vec<Arc<Route>> {
        self.routes_with_strategy.iter().map(|rws| rws.route.clone()).collect()
    }

    /// Get all routes with their resolved strategies (for middleware loading)
    pub fn routes_with_strategy(&self) -> &[RouteWithStrategy] {
        &self.routes_with_strategy
    }

    /// Get all unique active middleware names used by strategies in this site (excludes Off middleware)
    pub fn get_active_middleware_names(&self) -> Vec<String> {
        use std::collections::HashSet;
        
        let mut middleware_names = HashSet::new();
        for route_with_strategy in &self.routes_with_strategy {
            for middleware_config in route_with_strategy.active_strategy.middleware.iter() {
                match middleware_config {
                    crate::config::strategy::MiddlewareConfig::Named { name, .. }
                    | crate::config::strategy::MiddlewareConfig::On { name } => {
                        middleware_names.insert(name.clone());
                    },
                    crate::config::strategy::MiddlewareConfig::Off { name: _ } => {
                        // Skip Off middleware - they are disabled
                    },
                }
            }
        }
        
        let mut result: Vec<String> = middleware_names.into_iter().collect();
        result.sort(); // Сортируем для консистентности
        result
    }

    /// Get site primary domain
    pub fn site_name(&self) -> &str {
        &self.site_config.domain
    }

    /// Add TLS mapping for this site
    pub fn add_tls_mapping(&mut self, mapping: TlsMapping) {
        self.tls_mappings.push(mapping);
    }

    /// Get all TLS mappings for this site
    pub fn tls_mappings(&self) -> &[TlsMapping] {
        &self.tls_mappings
    }

    /// Get TLS mappings as a list (for compatibility with existing code)
    pub fn get_tls_list(&self) -> Vec<TlsMapping> {
        self.tls_mappings.clone()
    }

    /// Get active strategy middleware config by name for the given path
    pub fn get_active_strategy_config_by_route(&self, path: &str, middleware_name: &str) -> Result<Option<crate::config::strategy::MiddlewareConfig>, SiteManagerError> {
        let matched_route = self.get_route(path)?;
        let route_with_strategy = RouteWithStrategy::new(matched_route.route, matched_route.active_strategy);
        Ok(route_with_strategy.middleware_config(middleware_name).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Match, SiteConfig};

    fn create_test_site() -> SiteConfig {
        SiteConfig {
            domain: "test-site".to_string(),
            domains: vec![],
            listeners: vec![],
            routes: vec![
                Route::Proxy {
                    r#match: Match {
                        path: Some("/api/users/{id}".to_string()),
                        path_regex: None,
                    },
                    backend: "http://backend:8080".to_string(),
                    strategy: None,
                    strategies: None,
                },
                Route::Proxy {
                    r#match: Match {
                        path: None,
                        path_regex: Some(r"^/([^/]+)/final$".to_string()),
                    },
                    backend: "http://zerex222.ru:8080/{1}".to_string(),
                    strategy: None,
                    strategies: None,
                },
            ],
            strategy: None,
            strategies: std::collections::HashMap::new(),
        }
    }

    #[cfg(test)]
    mod strategy_resolver_tests {
        use super::*;
        use crate::config::strategy::{MiddlewareConfig, StrategyRef};
        use crate::config::{StrategyCollection, Match, GlobalConfig};
        use serde_json::json;

        fn create_test_global_config() -> GlobalConfig {
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
                proxy_id: "httpward".to_string(),
                sites_enabled: Default::default(),
                strategy: Some(StrategyRef::Named("default".to_string())),
                strategies,
            }
        }

        fn create_test_site_config_with_strategies() -> SiteConfig {
            let mut strategies = StrategyCollection::new();
            strategies.insert(
                "site_default".to_string(),
                vec![
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({"requests": 500}) // Missing "window" - should inherit
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
                        r#match: Match { path: Some("/api".to_string()), ..Default::default() },
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
        fn test_get_route_with_strategy() {
            let global_config = create_test_global_config();
            let site_config = Arc::new(create_test_site_config_with_strategies());
            let site_manager = SiteManager::new(site_config, Some(&global_config)).unwrap();

            let matched = site_manager.get_route("/api").unwrap();

            // Should have matched route
            assert_eq!(matched.matcher_type, MatcherType::Path);

            // Should have resolved strategy
            assert_eq!(matched.active_strategy.name, "site_default");
            assert_eq!(matched.active_strategy.middleware.len(), 3); // rate_limit + auth + logging (inherited)
        }

        #[test]
        fn test_strategy_inheritance_in_resolver() {
            let global_config = create_test_global_config();
            let site_config = Arc::new(create_test_site_config_with_strategies());
            let site_manager = SiteManager::new(site_config, Some(&global_config)).unwrap();

            let matched = site_manager.get_route("/api").unwrap();

            // Check rate_limit middleware inherited "window" from global
            let rate_limit = matched.active_strategy.middleware
                .iter()
                .find(|m| m.name() == "rate_limit")
                .unwrap();
            let config = rate_limit.config_as_json().unwrap();
            assert_eq!(config["requests"], 500); // Site value
            assert_eq!(config["window"], "1m"); // Inherited from global
        }

        #[test]
        fn test_get_active_middleware_names() {
            let global_config = create_test_global_config();
            let site_config = Arc::new(create_test_site_config_with_strategies());
            let site_manager = SiteManager::new(site_config, Some(&global_config)).unwrap();

            let middleware_names = site_manager.get_active_middleware_names();
            
            // Should contain middleware names from the strategy: rate_limit, auth, logging
            assert_eq!(middleware_names.len(), 3);
            assert!(middleware_names.contains(&"rate_limit".to_string()));
            assert!(middleware_names.contains(&"auth".to_string()));
            assert!(middleware_names.contains(&"logging".to_string()));
        }

        #[test]
        fn test_get_active_middleware_names_excludes_off() {
            use crate::config::strategy::{MiddlewareConfig, StrategyRef};
            use crate::config::StrategyCollection;
            
            let global_config = create_test_global_config();
            
            // Create site config with Off middleware
            let mut strategies = StrategyCollection::new();
            strategies.insert(
                "mixed_strategy".to_string(),
                vec![
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({"requests": 100})
                    ),
                    MiddlewareConfig::Off { name: "logging".to_string() }, // Disabled middleware
                    MiddlewareConfig::new_named_json(
                        "cors".to_string(),
                        json!({"origins": "*"})
                    ),
                ]
            );

            let site_config = SiteConfig {
                domain: "test.example.com".to_string(),
                domains: vec![],
                listeners: vec![],
                routes: vec![
                    Route::Proxy {
                        r#match: Match { path: Some("/api".to_string()), ..Default::default() },
                        backend: "http://backend".to_string(),
                        strategy: Some(StrategyRef::Named("mixed_strategy".to_string())),
                        strategies: None,
                    }
                ],
                strategy: Some(StrategyRef::Named("mixed_strategy".to_string())),
                strategies,
            };

            let site_manager = SiteManager::new(Arc::new(site_config), Some(&global_config)).unwrap();
            let middleware_names = site_manager.get_active_middleware_names();
            
            // Should only contain active middleware, exclude "logging" which is Off
            assert_eq!(middleware_names.len(), 2);
            assert!(middleware_names.contains(&"rate_limit".to_string()));
            assert!(middleware_names.contains(&"cors".to_string()));
            assert!(!middleware_names.contains(&"logging".to_string())); // Should be excluded
        }

        #[test]
        fn test_get_active_middleware_names_includes_on() {
            use crate::config::strategy::{MiddlewareConfig, StrategyRef};
            use crate::config::StrategyCollection;

            let global_config = create_test_global_config();

            let mut strategies = StrategyCollection::new();
            strategies.insert(
                "mixed_strategy".to_string(),
                vec![
                    MiddlewareConfig::new_on("httpward_log_module".to_string()),
                    MiddlewareConfig::new_named_json(
                        "rate_limit".to_string(),
                        json!({"requests": 100})
                    ),
                    MiddlewareConfig::new_off("logging".to_string()),
                ]
            );

            let site_config = SiteConfig {
                domain: "test.example.com".to_string(),
                domains: vec![],
                listeners: vec![],
                routes: vec![
                    Route::Proxy {
                        r#match: Match { path: Some("/api".to_string()), ..Default::default() },
                        backend: "http://backend".to_string(),
                        strategy: Some(StrategyRef::Named("mixed_strategy".to_string())),
                        strategies: None,
                    }
                ],
                strategy: Some(StrategyRef::Named("mixed_strategy".to_string())),
                strategies,
            };

            let site_manager = SiteManager::new(Arc::new(site_config), Some(&global_config)).unwrap();
            let middleware_names = site_manager.get_active_middleware_names();

            assert_eq!(middleware_names.len(), 2);
            assert!(middleware_names.contains(&"httpward_log_module".to_string()));
            assert!(middleware_names.contains(&"rate_limit".to_string()));
            assert!(!middleware_names.contains(&"logging".to_string()));
        }

        #[test]
        fn test_get_active_strategy_config_by_route() {
            let global_config = create_test_global_config();
            let site_config = Arc::new(create_test_site_config_with_strategies());
            let site_manager = SiteManager::new(site_config, Some(&global_config)).unwrap();

            // Test finding specific middleware by name
            let rate_limit_config = site_manager.get_active_strategy_config_by_route("/api", "rate_limit").unwrap();
            assert!(rate_limit_config.is_some());
            assert_eq!(rate_limit_config.unwrap().name(), "rate_limit");

            let auth_config = site_manager.get_active_strategy_config_by_route("/api", "auth").unwrap();
            assert!(auth_config.is_some());
            assert_eq!(auth_config.unwrap().name(), "auth");

            let logging_config = site_manager.get_active_strategy_config_by_route("/api", "logging").unwrap();
            assert!(logging_config.is_some());
            assert_eq!(logging_config.unwrap().name(), "logging");

            // Test non-existent middleware
            let non_existent = site_manager.get_active_strategy_config_by_route("/api", "non_existent").unwrap();
            assert!(non_existent.is_none());

            // Should return error for non-existent route
            let result = site_manager.get_active_strategy_config_by_route("/nonexistent", "rate_limit");
            assert!(matches!(result, Err(SiteManagerError::NoMatch)));
        }
    }
}
