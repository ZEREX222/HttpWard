use httpward_core::config::SiteConfig;
use super::listener::ListenerKey;

/// Runtime server instance description.
/// Represents one real TCP server that must be started.
#[derive(Debug)]
pub struct ServerInstance<'a> {
    /// Socket bind information
    pub bind: ListenerKey,

    /// Sites attached to this server (virtual hosts)
    pub sites: Vec<&'a SiteConfig>,
}
