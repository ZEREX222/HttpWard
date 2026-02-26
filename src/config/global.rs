// src/config/global.rs
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use schemars::JsonSchema;

/// Global application configuration (loaded from httpward.yaml)
#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct GlobalConfig {
    /// Addresses to bind/listen on (e.g. "0.0.0.0:443")
    pub listen: Vec<String>,

    /// Default TLS settings applied when site doesn't override
    pub tls: TlsDefault,

    /// Logging level ("trace" | "debug" | "info" | "warn" | "error")
    pub log: LogConfig,

    /// Path to directory with per-site .yaml / .yml files
    pub sites_enabled: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct TlsDefault {
    pub enabled: bool,
    pub default_cert: PathBuf,
    pub default_key: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
pub struct LogConfig {
    pub level: String,
}
