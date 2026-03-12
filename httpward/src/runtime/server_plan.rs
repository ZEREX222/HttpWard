// server/server_plan.rs
use std::collections::HashMap;
use std::sync::Arc;
use httpward_core::config::{AppConfig, SiteConfig, Listener};
use crate::runtime::tls_provisioner;
use httpward_core::core::server_models::listener::ListenerKey;
use httpward_core::core::server_models::server_instance::ServerInstance;
use httpward_core::core::server_models::site_manager::{SiteManager, TlsPaths, TlsMapping};

pub fn build_server_plan(config: &AppConfig) -> Vec<ServerInstance> {
    let mut servers_map: HashMap<ListenerKey, Vec<SiteConfig>> = HashMap::new();

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

            let sites_vec = servers_map.entry(key).or_default();
            sites_vec.push(site.clone());
        }
    }

    // 2. Process Global Listeners directly (for localhost/system access) 
    // only if no sites are using them
    for listener in &config.global.listeners {
        let key = ListenerKey {
            host: listener.host.clone(),
            port: listener.port,
        };

        let _sites_vec = servers_map.entry(key).or_default();
    }

    // 3. Build final instances with compiled SiteManagers
    servers_map
        .into_iter()
        .map(|(key, sites)| {
            // Compile SiteManagers from SiteConfigs
            let mut site_managers = Vec::new();
            let mut compilation_errors = Vec::new();
            
            for site_config in sites {
                match SiteManager::new(Arc::new(site_config.clone())) {
                    Ok(mut site_manager) => {
                        // Add TLS mappings to the site manager
                        for listener in get_effective_listeners(&site_config, &config.global) {
                            if let Some(paths) = resolve_site_tls(&site_config, &listener) {
                                let domains = site_config.get_all_domains();
                                if !domains.is_empty() {
                                    site_manager.add_tls_mapping(TlsMapping {
                                        domains,
                                        paths,
                                    });
                                }
                            }
                        }
                        
                        site_managers.push(Arc::new(site_manager));
                    }
                    Err(e) => {
                        compilation_errors.push(format!("Failed to compile site manager: {}", e));
                        tracing::error!("Failed to compile site manager for site: {}", e);
                    }
                }
            }
            
            if !compilation_errors.is_empty() {
                tracing::warn!("Site manager compilation errors: {:?}", compilation_errors);
            }
            
            ServerInstance {
                bind: key,
                site_managers,
                global: config.global.clone(),
            }
        })
        .collect()
}


/// Get effective listeners for a site (site listeners or global fallback)
fn get_effective_listeners(site: &SiteConfig, global_config: &httpward_core::config::GlobalConfig) -> Vec<Listener> {
    if site.listeners.is_empty() {
        global_config.listeners.clone()
    } else {
        site.listeners.clone()
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
