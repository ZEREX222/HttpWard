use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::global::{Listener, Route};

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

    /// Optional site-specific listeners (overrides global listeners)
    #[serde(default)]
    pub listeners: Vec<Listener>,

    /// Site-level routing rules
    #[serde(default)]
    pub routes: Vec<Route>,
}
