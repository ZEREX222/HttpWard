mod runtime;

use httpward_core::config::load;

use tracing::{info, debug};
use tracing_subscriber::{EnvFilter};


use runtime::server_plan::build_server_plan;


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = load("httpward.yaml")?;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.global.log.level.to_string()));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    info!("HttpWard starting...");

    debug!("Global:");
    debug!("  listeners: {:?}", config.global.listeners);
    debug!("  sites_enabled: {:?}", config.global.sites_enabled);

    debug!("Loaded {} sites:", config.sites.len());
    for site in &config.sites {
        debug!("  • {} ({} routes)", site.domain, site.routes.len());
    }

    let servers = build_server_plan(&config);

    for server in &servers {
        debug!(
            "Will start server on {}:{} ({} sites attached)",
            server.bind.host,
            server.bind.port,
            server.sites.len()
        );
    }

    debug!("Hello from HttpWard!");

    Ok(())
}
