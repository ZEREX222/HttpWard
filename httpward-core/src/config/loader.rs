// src/config/loader.rs
use super::{GlobalConfig, SiteConfig};
use super::strategy::{StrategyCollection, MiddlewareConfig};
use anyhow::{Context, Result};
use glob::glob;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use schemars::JsonSchema;
use tracing::info;

/// Combined configuration in memory: global + all loaded sites
#[derive(Debug, Clone, JsonSchema)]
pub struct AppConfig {
    pub global: GlobalConfig,
    pub sites: Vec<SiteConfig>,
}

pub fn load(config_path: impl AsRef<Path>) -> Result<AppConfig> {
    let config_path = config_path.as_ref();

    // 1. Load global config
    let global_content =
        fs::read_to_string(&config_path).context("Cannot read httpward.yaml config file")?;

    println!("🔍 DEBUG: Raw YAML content:");
    println!("{}", global_content);
    println!("---");

    let mut global: GlobalConfig = serde_yaml::from_str(&global_content)
        .context("Cannot parse httpward.yaml config (YAML error)")?;

    // Check strategies before merging
    if !global.strategies.is_empty() {
        println!("🔍 DEBUG: Strategies in global config:");
        for (name, middleware) in &global.strategies {
            println!("  - {}: {} middleware items", name, middleware.len());
            for (i, mw) in middleware.iter().enumerate() {
                println!("    [{}]: {:?}", i, mw);
            }
        }
    }

    // 2. Load default strategies from strategies.yml and merge with existing strategies
    if let Some(strategies_from_file) = load_default_strategies(config_path.parent().unwrap_or(Path::new(".")))? {
        // Merge strategies from file with existing ones (global strategies take precedence)
        for (name, middleware) in strategies_from_file {
            if !global.strategies.contains_key(&name) {
                println!("🔍 DEBUG: Adding strategy from file: {}", name);
                global.strategies.insert(name, middleware);
            } else {
                println!("🔍 DEBUG: Strategy {} already exists in global config, keeping global version", name);
            }
        }

        info!("Merged {} strategies from strategies.yml", global.strategies.len());
    } else {
        println!("🔍 DEBUG: No strategies file found, using only global config strategies");
    }

    // Validate each listener
    for (index, listener) in global.listeners.iter().enumerate() {
        if listener.port == 0 && listener.tls.is_some() {
            panic!(
                "Config `{}`: listener #{} has invalid port 0. Please set up the port.",
                "httpward.yaml", index
            );
        }
    }

    // 3. Load per-site configs
    let sites_dir = &global.sites_enabled;
    let mut sites = Vec::new();

    if sites_dir.exists() {
        for pattern in ["*.yaml", "*.yml"] {
            let full_pattern = sites_dir.join(pattern);
            for entry in
                glob(full_pattern.to_str().unwrap_or_default()).context("glob pattern error")?
            {
                let path = entry.context("glob entry error")?;

                let content = fs::read_to_string(&path)
                    .context(format!("Cannot read site config: {:?}", path))?;

                let site: SiteConfig = serde_yaml::from_str(&content)
                    .context(format!("Cannot parse site config {:?}: YAML error", path))?;

                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("<unknown>");

                validate_site_config(&site, file_name).context(format!(
                    "Invalid site config {:?}: no domain specified",
                    path
                ))?;

                info!("Loaded site config: {}", file_name);
                sites.push(site);
            }
        }
    }

    Ok(AppConfig { global, sites })
}

fn validate_site_config(site: &SiteConfig, file_name: &str) -> Result<()> {
    if site.domain.is_empty() && site.domains.is_empty() {
        panic!(
            "Error in the config: `{}`, must have at least `domain` or one entry in `domains`",
            file_name
        );
    }

    // Validate each listener
    for (index, listener) in site.listeners.iter().enumerate() {
        if listener.port == 0 && listener.tls.is_some() {
            panic!(
                "Config `{}`: listener #{} has invalid port 0. Please set up the port.",
                file_name, index
            );
        }
    }

    Ok(())
}

/// Load default strategies from strategies.yml in the given directory
/// Returns None if strategies.yml doesn't exist or is empty
fn load_default_strategies(base_dir: &Path) -> Result<Option<StrategyCollection>> {
    let strategies_path = base_dir.join("strategies.yml");
    
    if !strategies_path.exists() {
        // Try strategies.yaml as fallback
        let strategies_yaml_path = base_dir.join("strategies.yaml");
        if !strategies_yaml_path.exists() {
            return Ok(None);
        }
        return load_strategies_from_file(&strategies_yaml_path);
    }
    
    load_strategies_from_file(&strategies_path)
}

/// Load strategies from a specific file
fn load_strategies_from_file(strategies_path: &PathBuf) -> Result<Option<StrategyCollection>> {
    println!("🔍 DEBUG: Attempting to load strategies from: {:?}", strategies_path);
    
    let content = fs::read_to_string(strategies_path)
        .with_context(|| format!("Cannot read strategies file: {:?}", strategies_path))?;
    
    println!("🔍 DEBUG: Raw strategies file content:");
    println!("{}", content);
    println!("---");
    
    // Parse the YAML content - strategies.yml is a direct map of strategy names to middleware arrays
    let strategies_map: HashMap<String, serde_yaml::Value> = serde_yaml::from_str(&content)
        .with_context(|| format!("Cannot parse strategies file {:?}: YAML error", strategies_path))?;
    
    println!("🔍 DEBUG: Parsed {} strategies from file", strategies_map.len());
    
    // Return None if no strategies are defined
    if strategies_map.is_empty() {
        println!("🔍 DEBUG: No strategies found in file");
        return Ok(None);
    }
    
    // Convert to StrategyCollection
    let mut strategies = StrategyCollection::new();
    for (name, value) in strategies_map {
        // Parse each strategy value as a vector of middleware configurations
        let middleware: Vec<MiddlewareConfig> = serde_yaml::from_value(value)
            .with_context(|| format!("Cannot parse strategy '{}'", name))?;
        
        println!("🔍 DEBUG: Parsed strategy '{}' with {} middleware items", name, middleware.len());
        strategies.insert(name, middleware);
    }
    
    println!("🔍 DEBUG: Successfully loaded {} strategies from file", strategies.len());
    info!("Loaded {} strategies from {:?}", strategies.len(), strategies_path);
    Ok(Some(strategies))
}
