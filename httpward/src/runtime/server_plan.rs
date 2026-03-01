// server/server_plan.rs
use std::collections::HashMap;
use httpward_core::config::{AppConfig, SiteConfig, Listener};
use crate::runtime::tls_provisioner;
use super::listener::ListenerKey;
use super::server_instance::{ServerInstance, TlsPaths, TlsMapping};

pub fn build_server_plan(config: &AppConfig) -> Vec<ServerInstance> {
    let mut servers_map: HashMap<ListenerKey, (Vec<SiteConfig>, Vec<TlsMapping>)> = HashMap::new();

    // 1. Process Sites (Inheriting from or overriding Global Listeners)
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

            let (sites_vec, tls_reg) = servers_map.entry(key).or_default();
            sites_vec.push(site.clone());

            if let Some(paths) = resolve_site_tls(site, listener) {
                tls_reg.push(TlsMapping {
                    domains: get_all_domains(site),
                    paths,
                });
            }
        }
    }

    // 2. Process Global Listeners directly (for localhost/system access)
    for listener in &config.global.listeners {
        let key = ListenerKey {
            host: listener.host.clone(),
            port: listener.port,
        };

        let (_, tls_reg) = servers_map.entry(key).or_default();

        // If a global listener has TLS but no sites are attached yet,
        // we provision it for local access.
        if let Some(paths) = resolve_global_listener_tls(listener) {
            // Check if we already have a mapping for these local domains
            let local_domains = vec!["localhost".to_string(), "127.0.0.1".to_string()];

            // Only add if not already present to avoid duplication
            if !tls_reg.iter().any(|m| m.domains.contains(&"localhost".to_string())) {
                tls_reg.push(TlsMapping {
                    domains: local_domains,
                    paths,
                });
            }
        }
    }

    // 3. Build final instances
    servers_map
        .into_iter()
        .map(|(key, (sites, tls_registry))| ServerInstance {
            bind: key,
            sites,
            tls_registry,
            global: config.global.clone(),
        })
        .collect()
}

fn get_all_domains(site: &SiteConfig) -> Vec<String> {
    let mut domains = Vec::with_capacity(1 + site.domains.len());
    if !site.domain.is_empty() {
        domains.push(site.domain.clone());
    }
    domains.extend(site.domains.iter().cloned());
    domains
}

/// Resolves TLS for a listener specifically used by a Global context (localhost)
fn resolve_global_listener_tls(listener: &Listener) -> Option<TlsPaths> {
    let tls_config = listener.tls.as_ref()?;

    if tls_config.self_signed {
        let local_domains = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        tls_provisioner::provision_self_signed(&local_domains)
            .ok()
            .map(|p| TlsPaths { cert: p.cert, key: p.key })
    } else {
        if tls_config.cert.as_os_str().is_empty() { return None; }
        Some(TlsPaths {
            cert: tls_config.cert.clone(),
            key: tls_config.key.clone(),
        })
    }
}

fn resolve_site_tls(site: &SiteConfig, listener: &Listener) -> Option<TlsPaths> {
    let tls_config = listener.tls.as_ref()?;

    if tls_config.self_signed {
        let domains = get_all_domains(site);
        if domains.is_empty() { return None; }

        tls_provisioner::provision_self_signed(&domains)
            .ok()
            .map(|p| TlsPaths { cert: p.cert, key: p.key })
    } else {
        if tls_config.cert.as_os_str().is_empty() { return None; }
        Some(TlsPaths {
            cert: tls_config.cert.clone(),
            key: tls_config.key.clone(),
        })
    }
}
