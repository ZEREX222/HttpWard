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

        let sites_vec = servers_map.entry(key).or_default();
        
        // If no sites are using this listener and TLS is configured, create a global site
        if sites_vec.is_empty() {
            if let Some(_paths) = resolve_global_listener_tls(&config.global, listener) {
                let _domains = get_global_listener_domains(&config.global);
                
                // Create a global site config for localhost access
                let mut global_site = config.global.to_site_config();
                global_site.listeners = vec![listener.clone()];
                
                // Add to sites to be processed
                sites_vec.push(global_site);
            }
        }
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
                                
                                // Use domains from site config, or fallback to localhost for global sites
                                let tls_domains = if domains.is_empty() {
                                    vec!["localhost".to_string(), "127.0.0.1".to_string()]
                                } else {
                                    domains
                                };
                                
                                site_manager.add_tls_mapping(TlsMapping {
                                    domains: tls_domains,
                                    paths,
                                });
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
        // Use localhost domains if site has no domains (for global sites)
        let tls_domains = if domains.is_empty() {
            vec!["localhost".to_string(), "127.0.0.1".to_string()]
        } else {
            domains
        };

        tls_provisioner::provision_self_signed(&tls_domains)
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
