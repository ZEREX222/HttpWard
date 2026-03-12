use std::collections::HashMap;
use std::sync::Arc;
use std::path::PathBuf;
use matchit::Router;
use regex::{Regex, RegexSet};
use thiserror::Error;
use crate::config::{SiteConfig, Route};

#[derive(Debug, Clone)]
pub struct TlsPaths {
    pub cert: PathBuf,
    pub key: PathBuf,
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

/// Matched route with parameters
#[derive(Debug, Clone)]
pub struct MatchedRoute {
    pub route: Arc<Route>,
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
    /// routes stored as Arc to avoid expensive clones
    routes: Vec<Arc<Route>>,
    /// TLS mappings for this site's domains
    tls_mappings: Vec<TlsMapping>,
}

impl SiteManager {
    /// Create a new site manager from site configuration
    pub fn new(site_config: Arc<SiteConfig>) -> Result<Self, SiteManagerError> {
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

        Ok(Self {
            site_config,
            path_router,
            regex_list,
            regex_set,
            routes: routes_arc,
            tls_mappings: Vec::new(),
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
            let route = self.routes[route_index].clone();
            let mut params = HashMap::with_capacity(matched.params.len());
            for (k, v) in matched.params.iter() {
                params.insert(k.to_string(), v.to_string());
            }

            return Ok(MatchedRoute {
                route,
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

                        return Ok(MatchedRoute {
                            route: self.routes[*route_index].clone(),
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

                    return Ok(MatchedRoute {
                        route: self.routes[*route_index].clone(),
                        params,
                        matcher_type: MatcherType::Regex,
                    });
                }
            }
        }

        Err(SiteManagerError::NoMatch)
    }

    /// Get all routes (for debugging)
    pub fn routes(&self) -> &[Arc<Route>] {
        &self.routes
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

    #[test]
    fn test_site_manager_creation() {
        let site_config = Arc::new(create_test_site());
        let site_manager = SiteManager::new(site_config.clone()).unwrap();

        assert_eq!(site_manager.site_name(), "test-site");
        assert_eq!(site_manager.routes().len(), 2);
        assert!(Arc::ptr_eq(&site_manager.site_config(), &site_config));
        
        // Test that routes are stored as Arc
        for route in site_manager.routes() {
            assert_eq!(Arc::strong_count(route), 1);
        }
    }

    #[test]
    fn test_path_matching() {
        let site_config = Arc::new(create_test_site());
        let site_manager = SiteManager::new(site_config).unwrap();

        let matched = site_manager.get_route("/api/users/123").unwrap();

        assert!(matches!(*matched.route, Route::Proxy { .. }));
        assert_eq!(matched.params.get("id"), Some(&"123".to_string()));
        assert_eq!(matched.matcher_type, MatcherType::Path);
    }

    #[test]
    fn test_regex_matching() {
        let site_config = Arc::new(create_test_site());
        let site_manager = SiteManager::new(site_config).unwrap();

        let matched = site_manager.get_route("/my/final").unwrap();

        assert!(matches!(*matched.route, Route::Proxy { .. }));
        assert_eq!(matched.params.get("1"), Some(&"my".to_string()));
        assert_eq!(matched.matcher_type, MatcherType::Regex);
    }

    #[test]
    fn test_no_match() {
        let site_config = Arc::new(create_test_site());
        let site_manager = SiteManager::new(site_config).unwrap();

        let result = site_manager.get_route("/nonexistent");
        assert!(matches!(result, Err(SiteManagerError::NoMatch)));
    }

    #[test]
    fn test_named_capture_groups() {
        let mut site_config = create_test_site();
        
        // Add a route with named capture groups
        site_config.routes.push(Route::Proxy {
            r#match: Match {
                path: None,
                path_regex: Some(r"^/users/(?P<user_id>\d+)/posts/(?P<post_id>\d+)$".to_string()),
            },
            backend: "http://backend:8080/users/{user_id}/posts/{post_id}".to_string(),
            strategy: None,
            strategies: None,
        });

        let site_config = Arc::new(site_config);
        let site_manager = SiteManager::new(site_config).unwrap();

        let matched = site_manager.get_route("/users/123/posts/456").unwrap();

        assert!(matches!(*matched.route, Route::Proxy { .. }));
        assert_eq!(matched.params.get("user_id"), Some(&"123".to_string()));
        assert_eq!(matched.params.get("post_id"), Some(&"456".to_string()));
        assert_eq!(matched.matcher_type, MatcherType::Regex);
    }

    #[test]
    fn test_regex_set_optimization() {
        let site_config = Arc::new(create_test_site());
        let site_manager = SiteManager::new(site_config).unwrap();

        // Verify that RegexSet is created when regex patterns exist
        assert!(site_manager.regex_set.is_some());
        assert!(!site_manager.regex_list.is_empty());

        // Test that multiple regex patterns can be matched efficiently
        let results: Vec<_> = [
            "/my/final",
            "/test/final", 
            "/another/final"
        ].iter().map(|&path| site_manager.get_route(path).is_ok()).collect();

        assert_eq!(results, vec![true, true, true]);
    }

    #[test]
    fn test_arc_route_sharing() {
        let site_config = Arc::new(create_test_site());
        let site_manager = SiteManager::new(site_config).unwrap();

        let matched1 = site_manager.get_route("/api/users/123").unwrap();
        let matched2 = site_manager.get_route("/api/users/456").unwrap();

        // Both should point to the same Arc<Route> (same route)
        assert!(Arc::ptr_eq(&matched1.route, &matched2.route));
        
        // But have different parameters
        assert_eq!(matched1.params.get("id"), Some(&"123".to_string()));
        assert_eq!(matched2.params.get("id"), Some(&"456".to_string()));
    }

    #[test] 
    fn test_hashmap_capacity_optimization() {
        let site_config = Arc::new(create_test_site());
        let site_manager = SiteManager::new(site_config).unwrap();

        let matched = site_manager.get_route("/api/users/123").unwrap();
        
        // Verify that the params HashMap was created with proper capacity
        // This is mainly to ensure the optimization is in place
        assert!(!matched.params.is_empty());
        assert_eq!(matched.params.len(), 1);
    }

}
