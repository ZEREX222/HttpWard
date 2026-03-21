// server/server_instance.rs
use crate::config::GlobalConfig;
use crate::core::server_models::listener::ListenerKey;
use crate::core::server_models::site_manager::SiteManager;
use std::sync::Arc;

/// Runtime server instance description.
#[derive(Debug, Clone)]
pub struct ServerInstance {
    pub bind: ListenerKey,
    /// List of compiled site managers assigned to this listener.
    pub site_managers: Vec<Arc<SiteManager>>,
    pub global: GlobalConfig,
}
