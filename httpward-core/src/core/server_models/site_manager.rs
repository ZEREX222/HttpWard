use std::collections::HashMap;
use std::sync::Arc;
use matchit::Router;
use regex::Regex;
use thiserror::Error;
use crate::config::{SiteConfig, Route};

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
    pub route: Route,
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
    /// regex patterns with route indices
    regex_patterns: Vec<(Regex, usize)>,
    /// routes by index
    routes: Vec<Route>,
}

impl SiteManager {
    /// Create a new site manager from site configuration
    pub fn new(site_config: Arc<SiteConfig>) -> Result<Self, SiteManagerError> {
        let routes = site_config.routes.clone();
        let mut path_router = Router::new();
        let mut regex_patterns = Vec::new();
        
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
            
            // Add regex pattern if present
            if let Some(path_regex) = &match_config.path_regex {
                let regex = Regex::new(path_regex)
                    .map_err(|e| SiteManagerError::InvalidRegex(format!("{}: {}", path_regex, e)))?;
                regex_patterns.push((regex, index));
            }
        }
        
        Ok(Self {
            site_config,
            path_router,
            regex_patterns,
            routes,
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
            let route = &self.routes[route_index];
            let params = matched.params.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
                
            return Ok(MatchedRoute {
                route: route.clone(),
                params,
                matcher_type: MatcherType::Path,
            });
        }
        
        // Then try regex patterns (slower but more flexible)
        for (regex, route_index) in &self.regex_patterns {
            if let Some(captures) = regex.captures(path) {
                let route = &self.routes[*route_index];
                let params = captures.iter()
                    .enumerate()
                    .skip(1) // Skip the full match
                    .filter_map(|(i, cap)| {
                        cap.map(|m| (i.to_string(), m.as_str().to_string()))
                    })
                    .collect();
                    
                return Ok(MatchedRoute {
                    route: route.clone(),
                    params,
                    matcher_type: MatcherType::Regex,
                });
            }
        }
        
        Err(SiteManagerError::NoMatch)
    }
    
    /// Get all routes (for debugging)
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }
    
    /// Get site primary domain
    pub fn site_name(&self) -> &str {
        &self.site_config.domain
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
    }

    #[test]
    fn test_path_matching() {
        let site_config = Arc::new(create_test_site());
        let site_manager = SiteManager::new(site_config).unwrap();
        
        let matched = site_manager.get_route("/api/users/123").unwrap();
        
        assert!(matches!(matched.route, Route::Proxy { .. }));
        assert_eq!(matched.params.get("id"), Some(&"123".to_string()));
        assert_eq!(matched.matcher_type, MatcherType::Path);
    }

    #[test]
    fn test_regex_matching() {
        let site_config = Arc::new(create_test_site());
        let site_manager = SiteManager::new(site_config).unwrap();
        
        let matched = site_manager.get_route("/my/final").unwrap();
        
        assert!(matches!(matched.route, Route::Proxy { .. }));
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

}
