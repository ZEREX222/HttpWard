use std::collections::HashMap;
use matchit::Router;
use regex::Regex;
use thiserror::Error;
use httpward_core::config::Route;

#[derive(Error, Debug)]
pub enum MatcherError {
    #[error("invalid regex pattern: {0}")]
    InvalidRegex(String),
    #[error("invalid path pattern: {0}")]
    InvalidPath(String),
    #[error("no route matched")]
    NoMatch,
}

/// Route matcher that combines matchit (for path patterns) and regex (for path_regex)
#[derive(Debug, Clone)]
pub struct RouteMatcher {
    /// matchit router for path patterns
    path_router: Router<usize>,
    /// regex patterns with route indices
    regex_patterns: Vec<(Regex, usize)>,
    /// routes by index
    routes: Vec<Route>,
}

impl RouteMatcher {
    /// Create a new matcher from a list of routes
    pub fn new(routes: Vec<Route>) -> Result<Self, MatcherError> {
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
                    .map_err(|e| MatcherError::InvalidPath(format!("{}: {}", path, e)))?;
            }
            
            // Add regex pattern if present
            if let Some(path_regex) = &match_config.path_regex {
                let regex = Regex::new(path_regex)
                    .map_err(|e| MatcherError::InvalidRegex(format!("{}: {}", path_regex, e)))?;
                regex_patterns.push((regex, index));
            }
        }
        
        Ok(Self {
            path_router,
            regex_patterns,
            routes,
        })
    }
    
    /// Find matching route for the given path
    pub fn match_route(&self, path: &str) -> Result<MatchedRoute, MatcherError> {
        // First try matchit path patterns
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
        
        // Then try regex patterns
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
        
        Err(MatcherError::NoMatch)
    }
    
    /// Get all routes (for debugging)
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpward_core::config::{Match, Route};

    #[test]
    fn test_path_matching() {
        let routes = vec![
            Route::Proxy {
                r#match: Match {
                    path: Some("/api/users/{id}".to_string()),
                    path_regex: None,
                },
                backend: "http://backend:8080".to_string(),
            },
        ];
        
        let matcher = RouteMatcher::new(routes).unwrap();
        let matched = matcher.match_route("/api/users/123").unwrap();
        
        assert!(matches!(matched.route, Route::Proxy { .. }));
        assert_eq!(matched.params.get("id"), Some(&"123".to_string()));
        assert_eq!(matched.matcher_type, MatcherType::Path);
    }
    
    #[test]
    fn test_wildcard_in_middle_with_regex() {
        // Wildcard parameters can only be at the end with matchit, so use regex for middle wildcards
        let routes = vec![
            Route::Proxy {
                r#match: Match {
                    path: None,
                    path_regex: Some(r"^/([^/]+)/final$".to_string()),
                },
                backend: "http://zerex222.ru:8080/{1}".to_string(),
            },
        ];
        
        let matcher = RouteMatcher::new(routes).unwrap();
        let matched = matcher.match_route("/my/final").unwrap();
        
        assert!(matches!(matched.route, Route::Proxy { .. }));
        assert_eq!(matched.params.get("1"), Some(&"my".to_string()));
        assert_eq!(matched.matcher_type, MatcherType::Regex);
    }
    
    #[test]
    fn test_regex_matching() {
        let routes = vec![
            Route::Proxy {
                r#match: Match {
                    path: None,
                    path_regex: Some(r"^/api/users/(\d+)$".to_string()),
                },
                backend: "http://backend:8080".to_string(),
            },
        ];
        
        let matcher = RouteMatcher::new(routes).unwrap();
        let matched = matcher.match_route("/api/users/123").unwrap();
        
        assert!(matches!(matched.route, Route::Proxy { .. }));
        assert_eq!(matched.params.get("1"), Some(&"123".to_string()));
        assert_eq!(matched.matcher_type, MatcherType::Regex);
    }
}
