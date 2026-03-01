// server/server_instance.rs
use std::path::PathBuf;
use httpward_core::config::{SiteConfig, GlobalConfig};
use super::listener::ListenerKey;

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
#[derive(Debug)]
pub struct ServerInstance {
    pub bind: ListenerKey,
    /// List of raw site configurations assigned to this listener.
    pub sites: Vec<SiteConfig>,
    /// Resolved TLS mappings (domain -> cert/key) for this specific listener.
    pub tls_registry: Vec<TlsMapping>,
    pub global: GlobalConfig,
}
