use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use schemars::JsonSchema;

/// Configuration for one virtual host / site
#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SiteConfig {
    /// Primary domain name (used for SNI matching & logging)
    pub domain: String,

    /// Additional domain names / aliases
    #[serde(default)]
    pub domains: Vec<String>,

    /// List of listeners (each can have its own port and optional TLS)
    #[serde(default)]
    pub listeners: Vec<Listener>,

    /// List of routing rules
    #[serde(default)]
    pub routes: Vec<Route>,
}

/// A network listener for this site (port + optional TLS)
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Listener {
    /// TCP port to listen on (default: 443)
    #[serde(default = "default_port")]
    pub port: u16,

    /// Optional TLS certificate/key for this listener
    #[serde(default)]
    pub tls: Option<TlsOverride>,
}

fn default_port() -> u16 {
    80
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TlsOverride {
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
