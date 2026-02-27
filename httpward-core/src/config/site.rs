use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

use crate::config::global::{Listener, Route, Tls};

/// Configuration for one virtual host / site
#[derive(Debug, Clone, Deserialize, Serialize, Default, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SiteConfig {
    /// Primary domain name (used for SNI matching & logging)
    #[serde(default)]
    pub domain: String,

    /// Additional domain names / aliases
    #[serde(default)]
    pub domains: Vec<String>,

    /// Optional TLS override for this site (SNI-level override)
    #[serde(default)]
    pub tls: Option<Tls>,

    /// Optional site-specific listeners (overrides global listeners)
    #[serde(default)]
    pub listeners: Vec<Listener>,

    /// Site-level routing rules
    #[serde(default)]
    pub routes: Vec<Route>,
}
