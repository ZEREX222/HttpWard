// server/server_instance.rs
use std::path::PathBuf;
use httpward_core::config::{SiteConfig, GlobalConfig};
use super::listener::ListenerKey;

#[derive(Debug, Clone)]
pub struct TlsPaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// A site paired with its resolved TLS credentials
#[derive(Debug)]
pub struct SitePlan {
    pub config: SiteConfig,
    pub tls_paths: Option<TlsPaths>,
}

/// Runtime server instance description.
#[derive(Debug)]
pub struct ServerInstance {
    pub bind: ListenerKey,
    /// Now holds SitePlan instead of raw SiteConfig
    pub sites: Vec<SitePlan>,
    pub global: GlobalConfig,
}
