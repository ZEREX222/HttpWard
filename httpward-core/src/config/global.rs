use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use schemars::JsonSchema;

/// Global application configuration (loaded from httpward.yaml)
#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct GlobalConfig {
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
    pub cert: PathBuf,
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
    #[serde(default)]
    pub path_prefix: Option<String>,

    #[serde(default)]
    pub path: Option<String>,

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
