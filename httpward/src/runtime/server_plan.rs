use std::collections::{HashMap, HashSet};
use httpward_core::config::{AppConfig, SiteConfig};
use super::listener::ListenerKey;
use super::server_instance::ServerInstance;

/// Builds a runtime server plan from loaded configuration.
///
/// Rules:
/// 1. If site.listeners is empty → site uses global listeners.
/// 2. If site.listeners is NOT empty → they override global listeners.
/// 3. One server is created per unique (host, port).
/// 4. Global listeners without attached sites are also included.
pub fn build_server_plan<'a>(config: &'a AppConfig) -> Vec<ServerInstance<'a>> {
    // Map of bind key → list of sites attached to it
    let mut servers: HashMap<ListenerKey, Vec<&'a SiteConfig>> = HashMap::new();

    // === Attach sites to their effective listeners ===
    for site in &config.sites {
        // Determine which listeners apply to this site
        let effective_listeners = if site.listeners.is_empty() {
            &config.global.listeners
        } else {
            &site.listeners
        };

        for listener in effective_listeners {
            let key = ListenerKey {
                host: listener.host.clone(),
                port: listener.port,
            };

            servers
                .entry(key)
                .or_default()
                .push(site);
        }
    }

    // === Ensure global listeners without sites are still started ===
    let mut known_keys: HashSet<ListenerKey> =
        servers.keys().cloned().collect();

    for listener in &config.global.listeners {
        let key = ListenerKey {
            host: listener.host.clone(),
            port: listener.port,
        };

        if !known_keys.contains(&key) {
            servers.insert(key.clone(), Vec::new());
            known_keys.insert(key);
        }
    }

    // Convert HashMap into Vec<ServerInstance>
    servers
        .into_iter()
        .map(|(bind, sites)| ServerInstance { bind, sites })
        .collect()
}
