// server/server_plan.rs
use std::collections::HashMap;
use httpward_core::config::{AppConfig, SiteConfig, Listener};
use crate::runtime::tls_provisioner;
use super::listener::ListenerKey;
use super::server_instance::{ServerInstance, SitePlan, TlsPaths};

pub fn build_server_plan(config: &AppConfig) -> Vec<ServerInstance> {
    let mut servers_map: HashMap<ListenerKey, Vec<SitePlan>> = HashMap::new();

    // 1. Process each site and its effective listeners
    for site in &config.sites {
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

            // Resolve TLS specifically for this site on this listener
            let tls_paths = resolve_site_tls(site, listener);

            let site_plan = SitePlan {
                config: site.clone(),
                tls_paths,
            };

            servers_map.entry(key).or_default().push(site_plan);
        }
    }

    // 2. Add empty global listeners if they have no sites
    for listener in &config.global.listeners {
        let key = ListenerKey {
            host: listener.host.clone(),
            port: listener.port,
        };
        servers_map.entry(key).or_insert(Vec::new());
    }

    // 3. Build final instances
    servers_map
        .into_iter()
        .map(|(key, sites)| ServerInstance {
            bind: key,
            sites,
            global: config.global.clone(),
        })
        .collect()
}

fn resolve_site_tls(site: &SiteConfig, listener: &Listener) -> Option<TlsPaths> {
    let tls_config = listener.tls.as_ref()?;

    if tls_config.self_signed {
        // Collect domains for this specific site
        let mut domains = vec![site.domain.clone()];
        domains.extend(site.domains.clone());

        tls_provisioner::provision_self_signed(&domains)
            .ok()
            .map(|p| TlsPaths { cert: p.cert, key: p.key })
    } else {
        Some(TlsPaths {
            cert: tls_config.cert.clone(),
            key: tls_config.key.clone()
        })
    }
}
