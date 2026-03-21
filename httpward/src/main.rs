#![allow(dead_code)]

mod core;
mod runtime;
mod server;

use httpward_core::config::load;
use std::env;
use std::path::Path;
use std::sync::Arc;

use crate::core::middleware::basic::MiddlewareModuleLoadManager;
use crate::server::manager::HttpWardServerManager;
use httpward_core::core::server_models::server_instance::ServerInstance;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use runtime::server_plan::build_server_plan;
use server::http_server::HttpWardServer;
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

// !!! STATIC MODULES ONLY FOR DEBUG!!!
// Import static modules ONLY FOR DEBUG
#[cfg(feature = "static_modules")]
use httpward_log_module::HttpWardLogLayer;
#[cfg(feature = "static_modules")]
use httpward_rate_limit_module::HttpWardRateLimitLayer;

fn load_middleware_manager(
    server_plans: &[ServerInstance],
) -> Result<MiddlewareModuleLoadManager, Box<dyn std::error::Error + Send + Sync>> {
    // !!! STATIC MODULES ONLY FOR DEBUG IF YOU WANT TO DEBUG YOUR MIDDLEWARE MODULE!!!
    if cfg!(feature = "static_modules") {
        info!("Using static module loading");

        let static_modules = vec![
            // !!! ADD YOUR MIDDLEWARE MODULE HERE FOR LOCAL DEBUG !!!
            #[cfg(feature = "static_modules")]
            (
                "httpward_log_module",
                Arc::new(HttpWardLogLayer::new()) as Arc<dyn HttpWardMiddleware + Send + Sync>,
            ),
            #[cfg(feature = "static_modules")]
            (
                "httpward_rate_limit_module",
                Arc::new(HttpWardRateLimitLayer::new())
                    as Arc<dyn HttpWardMiddleware + Send + Sync>,
            ),
        ];

        MiddlewareModuleLoadManager::from_server_instances_statically(server_plans, static_modules)
    } else {
        info!("Using dynamic module loading");
        MiddlewareModuleLoadManager::from_server_instances(server_plans)
    }
}

fn find_config_file(base_path: &str) -> String {
    let path = Path::new(base_path);

    // If the path already has an extension, try it directly
    if let Some(extension) = path.extension()
        && (extension == "yaml" || extension == "yml")
            && path.exists() {
                return base_path.to_string();
            }

    // Try .yaml first, then .yml
    let yaml_path = format!("{}.yaml", base_path);
    let yml_path = format!("{}.yml", base_path);

    if Path::new(&yaml_path).exists() {
        yaml_path
    } else if Path::new(&yml_path).exists() {
        yml_path
    } else {
        // Default to .yaml if neither exists (let the error handling deal with it)
        yaml_path
    }
}

#[derive(Debug)]
struct Args {
    config: Option<String>,
    help: bool,
}

fn print_help() {
    println!("HttpWard - High Performance HTTP/HTTPS Reverse Proxy");
    println!();
    println!("USAGE:");
    println!("    httpward [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    --config <FILE>    Path to configuration file (default: httpward.yaml)");
    println!("    --help             Print this help information");
    println!();
    println!("EXAMPLES:");
    println!("    httpward                           # Use default config file");
    println!("    httpward --config myproxy.yaml     # Use custom config file");
    println!("    httpward --config /etc/proxy.yml   # Use absolute path");
    println!();
    println!("CONFIG FILE:");
    println!("    The config file can be in YAML (.yaml or .yml) format.");
    println!("    If no extension is provided, .yaml is tried first, then .yml.");
}

fn parse_args() -> Args {
    let args: Vec<String> = env::args().collect();
    let mut config = None;
    let mut help = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                if i + 1 < args.len() {
                    config = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --config requires a file path");
                    std::process::exit(1);
                }
            }
            "--help" | "-h" => {
                help = true;
                i += 1;
            }
            arg => {
                eprintln!("Error: Unknown argument: {}", arg);
                eprintln!("Use --help for usage information");
                std::process::exit(1);
            }
        }
    }

    Args { config, help }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = parse_args();

    // Handle --help flag
    if args.help {
        print_help();
        return Ok(());
    }

    let config_base_path = args.config.unwrap_or_else(|| "httpward".to_string());
    let config_path = find_config_file(&config_base_path);

    info!("Loading config from: {}", config_path);
    let config = load(&config_path)?;
    info!("Config loaded successfully from: {}", config_path);

    // Initialize logging with default level first
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.global.log.level));

    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Update logging level from config if needed
    // Note: In a real implementation, you might want to reconfigure the subscriber

    info!("HttpWard starting...");

    let server_plans = build_server_plan(&config);

    for server in &server_plans {
        let total_tls_mappings: usize = server
            .site_managers
            .iter()
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
            debug!("      Site {}: '{}'", i, site_manager.site_domains());
            debug!(
                "        Domains: {:?}",
                site_manager.site_config().get_all_domains()
            );
            debug!(
                "        Listeners: {} listeners",
                site_manager.site_config().listeners.len()
            );
            debug!(
                "        Routes: {} routes",
                site_manager.site_config().routes.len()
            );
        }

        // TLS details
        debug!("    TLS registry:");
        for (site_idx, site_manager) in server.site_managers.iter().enumerate() {
            for (i, tls_mapping) in site_manager.tls_mappings().iter().enumerate() {
                debug!(
                    "      TLS {}.{}: domains={:?}, cert={:?}, key={:?}",
                    site_idx, i, tls_mapping.domains, tls_mapping.paths.cert, tls_mapping.paths.key
                );
            }
        }

        debug!(""); // Empty line for readability
    }

    let manager_result = load_middleware_manager(&server_plans);

    if let Ok(manager) = manager_result {
        debug!(
            "Successfully loaded {} middleware modules",
            manager.module_count()
        );

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
