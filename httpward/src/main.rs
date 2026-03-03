mod runtime;
mod server;

use httpward_core::config::load;

use tracing::{info, debug};
use tracing_subscriber::{EnvFilter};
use runtime::server_plan::build_server_plan;
use server::http_server::HttpWardServer;
use crate::server::manager::ServerManager;

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
        debug!(
            "Will start server on {}:{} ({} sites attached / {} TLS attached)",
            server.bind.host,
            server.bind.port,
            server.sites.len(),
            server.tls_registry.len()
        );
        
        // Detailed server configuration
        debug!("  Server details:");
        debug!("    Bind: {}:{}", server.bind.host, server.bind.port);


        // Sites details
        debug!("    Sites attached:");
        for (i, site) in server.sites.iter().enumerate() {
            debug!("      Site {}: '{}'", i, site.domain);
            debug!("        Domains: {:?}", site.get_all_domains());
            debug!("        Listeners: {} listeners", site.listeners.len());
            debug!("        Routes: {} routes", site.routes.len());
        }

        // TLS details
        debug!("    TLS registry:");
        for (i, tls_mapping) in server.tls_registry.iter().enumerate() {
            debug!("      TLS {}: domains={:?}, cert={:?}, key={:?}", 
                i, tls_mapping.domains, tls_mapping.paths.cert, tls_mapping.paths.key);
        }
        
        debug!(""); // Empty line for readability
    }

    let mut instances = vec![];

    for plan in server_plans {
        // Create the HttpWardServer
        let server = HttpWardServer::new(plan);
        instances.push(server);
    }

    // 4. Run all servers concurrently
    ServerManager::start_all(instances).await?;

    debug!("Hello from HttpWard!");

    Ok(())
}
