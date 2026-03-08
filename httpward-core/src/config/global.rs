use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::site::SiteConfig;
use crate::config::strategy::{Strategy, StrategyRef, StrategyCollection};

/// Global application configuration (loaded from httpward.yaml)
/// Inherits all fields from SiteConfig plus global-specific settings
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
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

    /// Path to directory with per-site .yaml / .yml files
    #[serde(default)]
    pub sites_enabled: PathBuf,

    /// Default strategy for all domains and routes
    #[serde(default = "default_strategy")]
    pub strategy: String,

    /// Global strategy definitions
    #[serde(default)]
    pub strategies: StrategyCollection,
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
            strategy: Some(self.strategy.clone()),
            strategies: self.strategies.clone(),
        }
    }

    /// Get the default strategy
    pub fn get_default_strategy(&self) -> Option<Strategy> {
        self.strategies.get(&self.strategy).map(|middleware| Strategy {
            name: self.strategy.clone(),
            middleware: middleware.clone(),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tls {
    #[serde(default)]
    pub self_signed: bool,
    #[serde(default)]
    pub cert: PathBuf,
    #[serde(default)]
    pub key: PathBuf,
}

/// Single routing rule — proxy / static / redirect
#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Match {
    /// Using matchit library https://github.com/ibraheemdev/matchit
    #[serde(default)]
    pub path: Option<String>,

    /// Using basic regexp. Please use path if it's possible.
    #[serde(default)]
    pub path_regex: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Redirect {
    pub to: String,

    #[serde(default = "default_redirect_code")]
    pub code: u16,
}

fn default_redirect_code() -> u16 {
    301
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LogConfig {
    /// Logging level ("trace" | "debug" | "info" | "warn" | "error")
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String {
    "warn".to_string()
}

fn default_strategy() -> String {
    "default".to_string()
}
