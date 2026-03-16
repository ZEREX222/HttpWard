mod runtime;
mod server;
mod core;

use httpward_core::config::load;
use std::sync::Arc;

use tracing::{info, debug, warn};
use tracing_subscriber::{EnvFilter};
use runtime::server_plan::build_server_plan;
use server::http_server::HttpWardServer;
use crate::server::manager::HttpWardServerManager;
use crate::core::middleware::basic::MiddlewareModuleLoadManager;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::core::server_models::server_instance::ServerInstance;

// !!! STATIC MODULES ONLY FOR DEBUG!!!
// Import static modules ONLY FOR DEBUG
#[cfg(feature = "static_modules")]
use httpward_log_module::HttpWardLogLayer;

fn load_middleware_manager(server_plans: &[ServerInstance]) -> Result<MiddlewareModuleLoadManager, Box<dyn std::error::Error + Send + Sync>> {
    // !!! STATIC MODULES ONLY FOR DEBUG IF YOU WANT TO DEBUG YOUR MIDDLEWARE MODULE!!!
    if cfg!(feature = "static_modules") {
        info!("Using static module loading");

        let static_modules = vec![
            // !!! ADD YOUR MIDDLEWARE MODULE HERE FOR LOCAL DEBUG !!!
            #[cfg(feature = "static_modules")]
            ("httpward_log_module", Arc::new(HttpWardLogLayer::new()) as Arc<dyn HttpWardMiddleware + Send + Sync>)
        ];

        MiddlewareModuleLoadManager::from_server_instances_statically(server_plans, static_modules)
    } else {
        info!("Using dynamic module loading");
        MiddlewareModuleLoadManager::from_server_instances(server_plans)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = load("httpward.yaml")?;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.global.log.level.to_string()));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    info!("HttpWard starting...");

    let server_plans = build_server_plan(&config);
    
    for server in &server_plans {
        let total_tls_mappings: usize = server.site_managers.iter()
            .map(|sm| sm.tls_mappings().len())
            .sum();
        
        debug!(
            "Will start server on {}:{} ({} sites attached / {} TLS attached)",
            server.bind.host,
            server.bind.port,
            server.site_managers.len(),
            total_tls_mappings
        );
        
        // Detailed server configuration
        debug!("  Server details:");
        debug!("    Bind: {}:{}", server.bind.host, server.bind.port);


        // Sites details
        debug!("    Sites attached:");
        for (i, site_manager) in server.site_managers.iter().enumerate() {
            debug!("      Site {}: '{}'", i, site_manager.site_name());
            debug!("        Domains: {:?}", site_manager.site_config().get_all_domains());
            debug!("        Listeners: {} listeners", site_manager.site_config().listeners.len());
            debug!("        Routes: {} routes", site_manager.site_config().routes.len());
        }

        // TLS details
        debug!("    TLS registry:");
        for (site_idx, site_manager) in server.site_managers.iter().enumerate() {
            for (i, tls_mapping) in site_manager.tls_mappings().iter().enumerate() {
                debug!("      TLS {}.{}: domains={:?}, cert={:?}, key={:?}", 
                    site_idx, i, tls_mapping.domains, tls_mapping.paths.cert, tls_mapping.paths.key);
            }
        }
        
        debug!(""); // Empty line for readability
    }

    let manager_result = load_middleware_manager(&server_plans);

    if let Ok(manager) = manager_result {
        debug!("Successfully loaded {} middleware modules", manager.module_count());

        // Display loaded modules
        for module_name in manager.module_names() {
            debug!("  Loaded module: {}", module_name);
        }
    } else if let Err(e) = manager_result {
        panic!("Failed to load middleware modules: {}", e);
    }

    let mut instances = vec![];

    // Now create server instances from plans
    for plan in server_plans {
        // Create the HttpWardServer
        let server = HttpWardServer::new(plan);
        instances.push(server);
    }
    
    // 4. Run all servers concurrently
    HttpWardServerManager::start_all(instances).await?;

    Ok(())
}
