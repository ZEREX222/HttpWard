// src/config/loader.rs
use anyhow::{Context, Result};
use glob::glob;
use std::fs;
use std::path::{Path};
use schemars::JsonSchema;
use tracing::{info};
use super::{GlobalConfig, SiteConfig};

/// Combined configuration in memory: global + all loaded sites
#[derive(Debug, JsonSchema)]
pub struct AppConfig {
    pub global: GlobalConfig,
    pub sites: Vec<SiteConfig>,
}

pub fn load(config_path: impl AsRef<Path>) -> Result<AppConfig> {
    // 1. Load global config
    let global_content = fs::read_to_string(&config_path)
        .context("Cannot read httpward.yaml config file")?;

    let global: GlobalConfig = serde_yaml::from_str(&global_content)
        .context("Cannot parse httpward.yaml config (YAML error)")?;

    // 2. Load per-site configs
    let sites_dir = &global.sites_enabled;
    let mut sites = Vec::new();

    if sites_dir.exists() {
        for pattern in ["*.yaml", "*.yml"] {
            let full_pattern = sites_dir.join(pattern);
            for entry in glob(full_pattern.to_str().unwrap_or_default())
                .context("glob pattern error")?
            {
                let path = entry.context("glob entry error")?;

                let content = fs::read_to_string(&path)
                    .context(format!("Cannot read site config: {:?}", path))?;

                let site: SiteConfig = serde_yaml::from_str(&content)
                    .context(format!("Cannot parse site config {:?}: YAML error", path))?;

                let file_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("<unknown>");

                validate_site_config(&site, file_name)
                    .context(format!("Invalid site config {:?}: no domain specified", path))?;

                info!("Loaded site config: {}", file_name);
                sites.push(site);
            }
        }
    }

    Ok(AppConfig { global, sites })
}

fn validate_site_config(site: &SiteConfig, file_name: &str) -> Result<()> {
    if site.domain.is_empty() && site.domains.is_empty() {
        panic!("Error in the config: `{}`, must have at least `domain` or one entry in `domains`", file_name);
    }
    Ok(())
}
