use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use schemars::JsonSchema;
use crate::config::site::SiteConfig;
use crate::config::strategy::{Strategy, StrategyRef, LegacyStrategyCollection as StrategyCollection};

/// Global application configuration (loaded from httpward.yaml)
/// Inherits all fields from SiteConfig plus global-specific settings
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct GlobalConfig {
    /// Primary domain name (used for SNI matching & logging)
    #[serde(default)]
    pub domain: String,

    /// Additional domain names / aliases
    #[serde(default)]
    pub domains: Vec<String>,

    /// Network listeners (bind address + port + optional TLS)
    #[serde(default)]
    pub listeners: Vec<Listener>,

    /// Global routing rules (executed before site-level routes)
    #[serde(default)]
    pub routes: Vec<Route>,

    /// Logging configuration
    #[serde(default)]
    pub log: LogConfig,

    /// Proxy identifier used in Via header
    #[serde(default = "default_proxy_id")]
    pub proxy_id: String,

    /// Path to directory with per-site .yaml / .yml files
    #[serde(default)]
    pub sites_enabled: PathBuf,

    /// Default strategy for all domains and routes
    #[serde(default = "default_strategy")]
    pub strategy: Option<StrategyRef>,

    /// Global strategy definitions
    #[serde(default)]
    pub strategies: StrategyCollection,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            domain: String::default(),
            domains: Vec::default(),
            listeners: Vec::default(),
            routes: Vec::default(),
            log: LogConfig::default(),
            proxy_id: default_proxy_id(),
            sites_enabled: PathBuf::default(),
            strategy: default_strategy(),
            strategies: StrategyCollection::default(),
        }
    }
}

impl GlobalConfig {
    /// Get all domains for this global config (primary + additional)
    pub fn get_all_domains(&self) -> Vec<String> {
        let mut domains = Vec::with_capacity(1 + self.domains.len());
        if !self.domain.is_empty() {
            domains.push(self.domain.clone());
        }
        domains.extend(self.domains.iter().cloned());
        domains
    }

    /// Check if this global config has any domains configured
    pub fn has_domains(&self) -> bool {
        !self.domain.is_empty() || !self.domains.is_empty()
    }

    /// Convert global config to site config
    pub fn to_site_config(&self) -> SiteConfig {
        SiteConfig {
            domain: self.domain.clone(),
            domains: self.domains.clone(),
            listeners: self.listeners.clone(),
            routes: self.routes.clone(),
            strategy: self.strategy.clone(),
            strategies: self.strategies.clone(),
        }
    }

    /// Get the default strategy
    pub fn get_default_strategy(&self) -> Option<Strategy> {
        match &self.strategy {
            Some(strategy_ref) => strategy_ref.resolve(&self.strategies),
            None => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Listener {
    /// Bind address (default: 0.0.0.0)
    #[serde(default = "default_host")]
    pub host: String,

    /// TCP port
    #[serde(default)]
    pub port: u16,

    /// Optional TLS configuration
    #[serde(default)]
    pub tls: Option<Tls>,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Tls {
    #[serde(default)]
    pub self_signed: bool,
    #[serde(default)]
    pub cert: PathBuf,
    #[serde(default)]
    pub key: PathBuf,
}

/// Single routing rule — proxy / static / redirect
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Route {
    Proxy {
        #[serde(rename = "match")]
        r#match: Match,
        backend: String,
        #[serde(default)]
        strategy: Option<StrategyRef>,
        #[serde(default)]
        strategies: Option<StrategyCollection>,
    },
    Static {
        #[serde(rename = "match")]
        r#match: Match,
        static_dir: PathBuf,
        #[serde(default)]
        strategy: Option<StrategyRef>,
        #[serde(default)]
        strategies: Option<StrategyCollection>,
    },
    Redirect {
        #[serde(rename = "match")]
        r#match: Match,
        redirect: Redirect,
        #[serde(default)]
        strategy: Option<StrategyRef>,
        #[serde(default)]
        strategies: Option<StrategyCollection>,
    },
}

impl Route {
    /// Get the strategy reference for this route
    pub fn get_strategy(&self) -> Option<&StrategyRef> {
        match self {
            Route::Proxy { strategy, .. } => strategy.as_ref(),
            Route::Static { strategy, .. } => strategy.as_ref(),
            Route::Redirect { strategy, .. } => strategy.as_ref(),
        }
    }

    /// Get the strategy collection for this route
    pub fn get_strategies(&self) -> Option<&StrategyCollection> {
        match self {
            Route::Proxy { strategies, .. } => strategies.as_ref(),
            Route::Static { strategies, .. } => strategies.as_ref(),
            Route::Redirect { strategies, .. } => strategies.as_ref(),
        }
    }

    /// Get the match configuration for this route
    pub fn get_match(&self) -> &Match {
        match self {
            Route::Proxy { r#match, .. } => r#match,
            Route::Static { r#match, .. } => r#match,
            Route::Redirect { r#match, .. } => r#match,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct Match {
    /// Using matchit library https://github.com/ibraheemdev/matchit
    #[serde(default)]
    pub path: Option<String>,

    /// Using basic regexp. Please use path if it's possible.
    #[serde(default)]
    pub path_regex: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Redirect {
    pub to: String,

    #[serde(default = "default_redirect_code")]
    pub code: u16,
}

fn default_redirect_code() -> u16 {
    301
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct LogConfig {
    /// Logging level ("trace" | "debug" | "info" | "warn" | "error")
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String {
    "warn".to_string()
}

fn default_strategy() -> Option<StrategyRef> {
    Some(StrategyRef::Named("default".to_string()))
}

fn default_proxy_id() -> String {
    "httpward".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::strategy::StrategyRef;
    use crate::config::MiddlewareConfig;

    #[test]
    fn test_global_default_strategy() {
        let config = GlobalConfig::default();
        
        // Should have Some(StrategyRef::Named("default")) by default
        assert!(config.strategy.is_some(), "Strategy should be Some, got {:?}", config.strategy);
        
        match config.strategy.unwrap() {
            StrategyRef::Named(name) => assert_eq!(name, "default"),
            _ => panic!("Expected Named strategy"),
        }
    }

    #[test]
    fn test_global_strategy_resolution() {
        let mut config = GlobalConfig::default();
        
        // Add a default strategy to the collection
        config.strategies.insert("default".to_string(), vec![
            crate::config::strategy::MiddlewareConfig::new_named_json(
                "logging".to_string(),
                serde_json::json!({"level": "info"})
            )
        ]);
        
        // Should resolve the default strategy
        let resolved = config.get_default_strategy();
        assert!(resolved.is_some());
        
        let strategy = resolved.unwrap();
        assert_eq!(strategy.name, "default");
        assert_eq!(strategy.middleware.len(), 1);
        assert_eq!(strategy.middleware[0].name(), "logging");
    }

    #[test]
    fn test_global_inline_strategy() {
        let mut config = GlobalConfig::default();
        
        // Set an inline strategy using Strategy
        let inline_strategy = Strategy {
            name: "inline_test".to_string(),
            middleware: Arc::new(vec![
                crate::config::strategy::MiddlewareConfig::new_named_json(
                    "rate_limit".to_string(),
                    serde_json::json!({"requests": 1000, "window": "1m"})
                ),
                crate::config::strategy::MiddlewareConfig::new_named_json(
                    "logging".to_string(),
                    serde_json::json!({"level": "debug"})
                )
            ])
        };
        
        config.strategy = Some(StrategyRef::InlineMiddleware(vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                serde_json::json!({"requests": 1000, "window": "1m"})
            ),
            MiddlewareConfig::new_named_json(
                "logging".to_string(),
                serde_json::json!({"level": "debug"})
            )
        ]));
        
        // Should resolve the inline strategy
        let resolved = config.get_default_strategy();
        assert!(resolved.is_some());
        
        let strategy = resolved.unwrap();
        assert_eq!(strategy.name, "inline");
        assert_eq!(strategy.middleware.len(), 2);
        assert_eq!(strategy.middleware[0].name(), "rate_limit");
        assert_eq!(strategy.middleware[1].name(), "logging");
    }
}
