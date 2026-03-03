// server/server_plan.rs
use std::collections::HashMap;
use httpward_core::config::{AppConfig, SiteConfig, Listener};
use crate::runtime::tls_provisioner;
use super::listener::ListenerKey;
use super::server_instance::{ServerInstance, TlsPaths, TlsMapping};

pub fn build_server_plan(config: &AppConfig) -> Vec<ServerInstance> {
    let mut servers_map: HashMap<ListenerKey, (Vec<SiteConfig>, Vec<TlsMapping>)> = HashMap::new();

    // 1. Process Sites (including Global Config as a site if it has domains)
    let mut all_sites = config.sites.clone();
    
    // Add global config as a site if it has domains
    if config.global.has_domains() {
        all_sites.push(config.global.to_site_config());
    }

    for site in &all_sites {
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
                let domains = site.get_all_domains();
                
                // Only add TLS mapping if not already present for these domains
                if !tls_reg.iter().any(|m| m.domains.iter().any(|d| domains.contains(d))) {
                    tls_reg.push(TlsMapping {
                        domains,
                        paths,
                    });
                }
            }
        }
    }

    // 2. Process Global Listeners directly (for localhost/system access) 
    // only if no sites are using them
    for listener in &config.global.listeners {
        let key = ListenerKey {
            host: listener.host.clone(),
            port: listener.port,
        };

        let (sites_vec, tls_reg) = servers_map.entry(key).or_default();

        // Only add global listener TLS if no sites are attached and TLS is configured
        if sites_vec.is_empty() {
            if let Some(paths) = resolve_global_listener_tls(&config.global, listener) {
                let domains = get_global_listener_domains(&config.global);
                
                tls_reg.push(TlsMapping {
                    domains,
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

/// Get domains for a global listener - uses global domains if configured,
/// otherwise falls back to localhost domains for local access
fn get_global_listener_domains(global_config: &httpward_core::config::GlobalConfig) -> Vec<String> {
    if global_config.has_domains() {
        global_config.get_all_domains()
    } else {
        vec!["localhost".to_string(), "127.0.0.1".to_string()]
    }
}

/// Resolves TLS for a listener specifically used by a Global context
/// Uses global domains if configured, otherwise defaults to localhost
fn resolve_global_listener_tls(global_config: &httpward_core::config::GlobalConfig, listener: &Listener) -> Option<TlsPaths> {
    let tls_config = listener.tls.as_ref()?;

    if tls_config.self_signed {
        let domains = get_global_listener_domains(global_config);
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

fn resolve_site_tls(site: &SiteConfig, listener: &Listener) -> Option<TlsPaths> {
    let tls_config = listener.tls.as_ref()?;

    if tls_config.self_signed {
        let domains = site.get_all_domains();
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
