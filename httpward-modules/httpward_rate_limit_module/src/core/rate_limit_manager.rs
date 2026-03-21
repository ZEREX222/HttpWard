use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::Mutex;

use super::rate_limiter::{RateLimiter, RateLimitKeyKind, RateLimitScope, RouteScopeKey};

#[derive(Debug, Clone, Copy)]
pub struct SiteRateLimitSettings {
    pub max_entries: usize,
    pub idle_ttl: Duration,
    pub cleanup_interval: Duration,
}

impl Default for SiteRateLimitSettings {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            idle_ttl: Duration::from_secs(60),
            cleanup_interval: Duration::from_secs(10),
        }
    }
}

impl From<&super::httpward_rate_limit_config::InternalRateLimitStoreConfig> for SiteRateLimitSettings {
    fn from(value: &super::httpward_rate_limit_config::InternalRateLimitStoreConfig) -> Self {
        Self {
            max_entries: value.max_entries.max(1),
            idle_ttl: Duration::from_secs(value.idle_ttl_secs.max(1)),
            cleanup_interval: Duration::from_secs(value.cleanup_interval_secs.max(1)),
        }
    }
}

#[derive(Debug)]
struct SiteState {
    limiter: RateLimiter,
    global_rules_initialized: bool,
    initialized_route_scopes: HashSet<RouteScopeKey>,
}

impl SiteState {
    fn new(settings: SiteRateLimitSettings) -> Self {
        Self {
            limiter: RateLimiter::new(
                settings.max_entries,
                settings.idle_ttl,
                settings.cleanup_interval,
            ),
            global_rules_initialized: false,
            initialized_route_scopes: HashSet::new(),
        }
    }
}

/// Global thread-safe rate limit manager.
pub struct RateLimitManager {
    sites: Arc<RwLock<HashMap<String, Arc<Mutex<SiteState>>>>>,
}

impl RateLimitManager {
    pub fn new() -> Self {
        Self {
            sites: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn init_site(
        &self,
        site_name: &str,
        settings: SiteRateLimitSettings,
    ) -> Result<(), String> {
        let mut sites = self
            .sites
            .write()
            .map_err(|e| format!("Failed to acquire write lock on rate-limit sites: {e}"))?;

        sites
            .entry(site_name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(SiteState::new(settings))));

        Ok(())
    }

    async fn get_site_state(&self, site_name: &str) -> Result<Arc<Mutex<SiteState>>, String> {
        let sites = self
            .sites
            .read()
            .map_err(|e| format!("Failed to acquire read lock on rate-limit sites: {e}"))?;

        sites
            .get(site_name)
            .cloned()
            .ok_or_else(|| format!("Rate-limit site '{site_name}' was not initialized"))
    }

    fn get_site_state_sync(&self, site_name: &str) -> Result<Arc<Mutex<SiteState>>, String> {
        let sites = self
            .sites
            .read()
            .map_err(|e| format!("Failed to acquire read lock on rate-limit sites: {e}"))?;

        sites
            .get(site_name)
            .cloned()
            .ok_or_else(|| format!("Rate-limit site '{site_name}' was not initialized"))
    }

    /// Initialize a site and register route/global rules from config.
    pub async fn init_from_config(
        &self,
        site_name: &str,
        matched_route_scope: Option<RouteScopeKey>,
        config: &super::httpward_rate_limit_config::HttpWardRateLimitConfig,
    ) -> Result<(), String> {
        let internal = config.to_internal();
        self.init_site(site_name, SiteRateLimitSettings::from(&internal.store))?;

        let site = self.get_site_state(site_name).await?;
        let mut site = site.lock().await;

        if !internal.global.is_empty() && !site.global_rules_initialized {
            for rule in &internal.global {
                site.limiter.add_rule(
                    rule.key.clone(),
                    RateLimitScope::Global,
                    rule.to_runtime_rule(),
                );
            }

            site.global_rules_initialized = true;
        }

        if let Some(scope) = matched_route_scope {
            if !internal.matched_route.is_empty() && site.initialized_route_scopes.insert(scope) {
                for rule in &internal.matched_route {
                    site.limiter.add_rule(
                        rule.key.clone(),
                        RateLimitScope::Route(scope),
                        rule.to_runtime_rule(),
                    );
                }
            }
        }

        Ok(())
    }

    /// Synchronous variant used during middleware startup initialization.
    pub fn init_from_config_sync(
        &self,
        site_name: &str,
        matched_route_scope: Option<RouteScopeKey>,
        config: &super::httpward_rate_limit_config::HttpWardRateLimitConfig,
    ) -> Result<(), String> {
        let internal = config.to_internal();
        self.init_site(site_name, SiteRateLimitSettings::from(&internal.store))?;

        let site = self.get_site_state_sync(site_name)?;
        let mut site = site
            .try_lock()
            .map_err(|_| format!("Rate-limit site '{site_name}' state is busy during startup init"))?;

        if !internal.global.is_empty() && !site.global_rules_initialized {
            for rule in &internal.global {
                site.limiter.add_rule(
                    rule.key.clone(),
                    RateLimitScope::Global,
                    rule.to_runtime_rule(),
                );
            }

            site.global_rules_initialized = true;
        }

        if let Some(scope) = matched_route_scope {
            if !internal.matched_route.is_empty() && site.initialized_route_scopes.insert(scope) {
                for rule in &internal.matched_route {
                    site.limiter.add_rule(
                        rule.key.clone(),
                        RateLimitScope::Route(scope),
                        rule.to_runtime_rule(),
                    );
                }
            }
        }

        Ok(())
    }

    pub async fn check(
        &self,
        site_name: &str,
        kind: RateLimitKeyKind,
        scope: RateLimitScope,
        value: &str,
    ) -> Result<bool, String> {
        let site = self.get_site_state(site_name).await?;
        let mut site = site.lock().await;
        Ok(site.limiter.check(kind, scope, value))
    }

    pub async fn check_all(
        &self,
        site_name: &str,
        checks: &[(RateLimitKeyKind, RateLimitScope, String)],
    ) -> Result<bool, String> {
        let site = self.get_site_state(site_name).await?;
        let mut site = site.lock().await;

        for (kind, scope, value) in checks {
            if !site.limiter.check(kind.clone(), scope.clone(), value) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub async fn cleanup_site(&self, site_name: &str) -> Result<(), String> {
        let site = self.get_site_state(site_name).await?;
        let mut site = site.lock().await;
        site.limiter.cleanup();
        Ok(())
    }

    pub async fn cleanup_all(&self) -> Result<(), String> {
        let sites = self
            .sites
            .read()
            .map_err(|e| format!("Failed to acquire read lock on rate-limit sites: {e}"))?;

        let site_states: Vec<_> = sites.values().cloned().collect();
        drop(sites);

        for site in site_states {
            let mut site = site.lock().await;
            site.limiter.cleanup();
        }

        Ok(())
    }

    pub async fn stats(&self, site_name: &str) -> Result<RateLimiterStats, String> {
        let site = self.get_site_state(site_name).await?;
        let site = site.lock().await;

        Ok(RateLimiterStats {
            bucket_count: site.limiter.bucket_count(),
            rule_count: site.limiter.rule_count(),
            initialized_route_scope_count: site.initialized_route_scopes.len(),
            global_rules_initialized: site.global_rules_initialized,
        })
    }
}

impl Default for RateLimitManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiterStats {
    pub bucket_count: usize,
    pub rule_count: usize,
    pub initialized_route_scope_count: usize,
    pub global_rules_initialized: bool,
}

static RATE_LIMIT_MANAGER: OnceLock<RateLimitManager> = OnceLock::new();

pub fn init_global_manager() -> &'static RateLimitManager {
    RATE_LIMIT_MANAGER.get_or_init(RateLimitManager::new)
}

pub fn get_global_manager() -> Option<&'static RateLimitManager> {
    RATE_LIMIT_MANAGER.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimitStrategy;

    #[tokio::test]
    async fn test_manager_initializes_site_once() {
        let manager = RateLimitManager::new();

        manager
            .init_site(
                "test.local",
                SiteRateLimitSettings {
                    max_entries: 1_000,
                    idle_ttl: Duration::from_secs(60),
                    cleanup_interval: Duration::from_secs(10),
                },
            )
            .unwrap();

        manager
            .init_site(
                "test.local",
                SiteRateLimitSettings {
                    max_entries: 5,
                    idle_ttl: Duration::from_secs(1),
                    cleanup_interval: Duration::from_secs(1),
                },
            )
            .unwrap();

        let stats = manager.stats("test.local").await.unwrap();
        assert_eq!(stats.rule_count, 0);
    }

    #[tokio::test]
    async fn test_manager_registers_global_and_route_rules() {
        use crate::core::{HttpWardRateLimitConfig, RateLimitRuleConfig, RateLimitStoreConfig};
        use std::collections::HashMap;

        let manager = RateLimitManager::new();
        let route_marker = std::sync::Arc::new(1usize);
        let route_key = RouteScopeKey::from_arc_ptr(&route_marker);

        let mut global_rules = HashMap::new();
        global_rules.insert(
            "ip".to_string(),
            RateLimitRuleConfig {
                max_requests: 2,
                window: "1s".to_string(),
                strategy: RateLimitStrategy::Sliding, // Explicitly set for test
            },
        );

        let mut route_rules = HashMap::new();
        route_rules.insert(
            "ip".to_string(),
            RateLimitRuleConfig {
                max_requests: 1,
                window: "1s".to_string(),
                strategy: RateLimitStrategy::Sliding, // Explicitly set for test
            },
        );

        let config = HttpWardRateLimitConfig {
            global_config: Some(RateLimitStoreConfig::default()),
            global_rules: vec![global_rules],
            current_site_rules: vec![route_rules],
            response: None,
        };

        manager
            .init_from_config("test.local", Some(route_key), &config)
            .await
            .unwrap();

        let checks = vec![
            (
                RateLimitKeyKind::Ip,
                RateLimitScope::Global,
                "127.0.0.1".to_string(),
            ),
            (
                RateLimitKeyKind::Ip,
                RateLimitScope::Route(route_key),
                "127.0.0.1".to_string(),
            ),
        ];

        assert!(manager.check_all("test.local", &checks).await.unwrap());
        assert!(!manager.check_all("test.local", &checks).await.unwrap());
    }
}


