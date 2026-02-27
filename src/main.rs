mod config;
use config::load;
use anyhow::Context;

use tracing::{info, warn, error, debug};
use tracing_subscriber::{EnvFilter};

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

    debug!("Hello from HttpWard!");

    Ok(())
}
