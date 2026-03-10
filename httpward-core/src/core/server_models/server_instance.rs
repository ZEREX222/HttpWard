// server/server_instance.rs
use std::path::PathBuf;
use crate::config::{SiteConfig, GlobalConfig};
use crate::core::server_models::listener::ListenerKey;

#[derive(Debug, Clone)]
pub struct TlsPaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// A mapping between a set of domains and their specific certificate files.
/// Used for SNI (Server Name Indication) lookup during the TLS handshake.
#[derive(Debug, Clone)]
pub struct TlsMapping {
    pub domains: Vec<String>,
    pub paths: TlsPaths,
}

/// Runtime server instance description.
#[derive(Debug, Clone)]
pub struct ServerInstance {
    pub bind: ListenerKey,
    /// List of raw site configurations assigned to this listener.
    pub sites: Vec<SiteConfig>,
    /// Resolved TLS mappings (domain -> cert/key) for this specific listener.
    pub tls_registry: Vec<TlsMapping>,
    pub global: GlobalConfig,
}
