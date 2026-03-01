mod runtime;
mod server;

use httpward_core::config::load;

use tracing::{info, debug};
use tracing_subscriber::{EnvFilter};
use httpward_core::middleware::{LoggerMiddleware, Middleware};
use runtime::server_plan::build_server_plan;
use server::http_server::HttpServer;
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

    debug!("Global:");
    debug!("  listeners: {:?}", config.global.listeners);
    debug!("  sites_enabled: {:?}", config.global.sites_enabled);

    debug!("Loaded {} sites:", config.sites.len());
    for site in &config.sites {
        debug!("  • {} ({} routes)", site.domain, site.routes.len());
    }

    let server_plans = build_server_plan(&config);

    for server in &server_plans {
        debug!(
            "Will start server on {}:{} ({} sites attached / {} TLS attached)",
            server.bind.host,
            server.bind.port,
            server.sites.len(),
            server.sites.iter().map(|s| s.tls_paths.is_some()).count()
            )
    }

    let mut instances = vec![];

    for plan in server_plans {
        // 2. Define Middlewares for this specific server instance
        let mut pipeline: Vec<Box<dyn Middleware>> = vec![];
        pipeline.push(Box::new(LoggerMiddleware));
        // pipeline.push(Box::new(AuthMiddleware::new()));

        // 3. Create the HttpServer
        let server = HttpServer::new(
            plan,
            pipeline
        );

        instances.push(server);
    }

    // 4. Run all servers concurrently
    ServerManager::start_all(instances).await?;

    debug!("Hello from HttpWard!");

    Ok(())
}
