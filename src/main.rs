mod config;
use config::load;
use anyhow::Context;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("HttpWard starting...");

    let config = load("httpward.yaml")
        .context("Failed to load configuration")?;

    println!("\nGlobal:");
    println!("  listen: {:?}", config.global.listen);
    println!("  sites_enabled: {:?}", config.global.sites_enabled);

    println!("\nLoaded {} sites:", config.sites.len());
    for site in &config.sites {
        println!("  • {} ({} routes)", site.domain, site.routes.len());
    }

    println!("\nHello from HttpWard!");

    // Дальше будет сервер...

    Ok(())
}
