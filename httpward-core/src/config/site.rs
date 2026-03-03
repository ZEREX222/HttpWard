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

impl SiteConfig {
    /// Get all domains for this site config (primary + additional)
    pub fn get_all_domains(&self) -> Vec<String> {
        let mut domains = Vec::with_capacity(1 + self.domains.len());
        if !self.domain.is_empty() {
            domains.push(self.domain.clone());
        }
        domains.extend(self.domains.iter().cloned());
        domains
    }

    /// Check if this site config has any domains configured
    pub fn has_domains(&self) -> bool {
        !self.domain.is_empty() || !self.domains.is_empty()
    }
}
