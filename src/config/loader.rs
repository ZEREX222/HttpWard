// src/config/loader.rs
use anyhow::{Context, Result};
use glob::glob;
use std::fs;
use std::path::{Path, PathBuf};
use schemars::JsonSchema;
use super::{GlobalConfig, SiteConfig};

/// Combined configuration in memory: global + all loaded sites
#[derive(Debug, JsonSchema)]
pub struct AppConfig {
    pub global: GlobalConfig,
    pub sites: Vec<SiteConfig>,
}

/*pub fn load(config_path: impl AsRef<Path>) -> Result<AppConfig> {
    // 1. Load global config
    let global_content = fs::read_to_string(&config_path)
        .context("Cannot read global config file")?;

    let mut global: GlobalConfig = serde_yaml::from_str(&global_content)
        .context("Cannot parse global config (YAML error)")?;

    // Fallback for sites_enabled if not set
    if global.sites_enabled.as_os_str().is_empty() {
        global.sites_enabled = PathBuf::from("./sites-enabled");
    }

    // 2. Load per-site configs
    let sites_dir = &global.sites_enabled;
    let mut sites = Vec::new();

    if !sites_dir.exists() {
        println!("Warning: sites_enabled directory not found: {:?}", sites_dir);
    } else {
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

                println!("Loaded site config: {}", site.domain);
                sites.push(site);
            }
        }
    }

    Ok(AppConfig { global, sites })
}*/
