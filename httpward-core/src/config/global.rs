use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use schemars::JsonSchema;

use crate::config::site::SiteConfig;

/// Global application configuration (loaded from httpward.yaml)
/// Inherits all fields from SiteConfig plus global-specific settings
#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
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
    },
    Static {
        #[serde(rename = "match")]
        r#match: Match,
        static_dir: PathBuf,
    },
    Redirect {
        #[serde(rename = "match")]
        r#match: Match,
        redirect: Redirect,
    },
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
