use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use super::rate_limiter::{
    RateLimitCheckResults, RateLimitKeyKind, RateLimitScope, RateLimiter, RouteScopeKey,
};

#[derive(Debug, Clone, Copy)]
pub struct RateLimitSettings {
    pub max_entries: usize,
    pub idle_ttl: Duration,
    pub cleanup_interval: Duration,
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            idle_ttl: Duration::from_secs(60),
            cleanup_interval: Duration::from_secs(10),
        }
    }
}

impl From<&super::httpward_rate_limit_config::InternalRateLimitStoreConfig> for RateLimitSettings {
    fn from(value: &super::httpward_rate_limit_config::InternalRateLimitStoreConfig) -> Self {
        Self {
            max_entries: value.max_entries.max(1),
            idle_ttl: Duration::from_secs(value.idle_ttl_secs.max(1)),
            cleanup_interval: Duration::from_secs(value.cleanup_interval_secs.max(1)),
        }
    }
}

#[derive(Debug)]
struct ManagerState {
    limiter: RateLimiter,
    settings_initialized: bool,
    global_rules_initialized: bool,
    initialized_route_scopes: HashSet<RouteScopeKey>,
}

impl ManagerState {
    fn new(settings: RateLimitSettings) -> Self {
        Self {
            limiter: RateLimiter::new(
                settings.max_entries,
                settings.idle_ttl,
                settings.cleanup_interval,
            ),
            settings_initialized: false,
            global_rules_initialized: false,
            initialized_route_scopes: HashSet::new(),
        }
    }
}

/// Global thread-safe rate limit manager.
pub struct RateLimitManager {
    state: Arc<Mutex<ManagerState>>,
}

impl RateLimitManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ManagerState::new(RateLimitSettings::default()))),
        }
    }

    async fn init_settings_from_store_once(
        &self,
        store: &super::httpward_rate_limit_config::InternalRateLimitStoreConfig,
    ) {
        let mut state = self.state.lock().unwrap();
        if !state.settings_initialized {
            let settings = RateLimitSettings::from(store);
            state.limiter = RateLimiter::new(
                settings.max_entries,
                settings.idle_ttl,
                settings.cleanup_interval,
            );
            state.settings_initialized = true;
        }
    }

    fn lock_state_sync(&self) -> Result<std::sync::MutexGuard<'_, ManagerState>, String> {
        self.state.lock().map_err(|e| e.to_string())
    }

    /// Initialize manager and register route/global rules from config.
    pub async fn init_from_config(
        &self,
        matched_route_scope: Option<RouteScopeKey>,
        config: &super::httpward_rate_limit_config::HttpWardRateLimitConfig,
    ) -> Result<(), String> {
        let internal = config.to_internal();
        self.init_settings_from_store_once(&internal.store).await;

        let mut state = self.state.lock().map_err(|e| e.to_string())?;

        if !internal.global.is_empty() && !state.global_rules_initialized {
            for rule in &internal.global {
                state.limiter.add_rule(
                    rule.key.clone(),
                    RateLimitScope::Global,
                    rule.to_runtime_rule(),
                );
            }

            state.global_rules_initialized = true;
        }

        if let Some(scope) = matched_route_scope
            && !internal.matched_route.is_empty()
            && state.initialized_route_scopes.insert(scope)
        {
            for rule in &internal.matched_route {
                state.limiter.add_rule(
                    rule.key.clone(),
                    RateLimitScope::Route(scope),
                    rule.to_runtime_rule(),
                );
            }
        }

        Ok(())
    }

    /// Synchronous variant used during middleware startup initialization.
    pub fn init_from_config_sync(
        &self,
        matched_route_scope: Option<RouteScopeKey>,
        config: &super::httpward_rate_limit_config::HttpWardRateLimitConfig,
    ) -> Result<(), String> {
        let internal = config.to_internal();
        let mut state = self.lock_state_sync()?;

        if !state.settings_initialized {
            let settings = RateLimitSettings::from(&internal.store);
            state.limiter = RateLimiter::new(
                settings.max_entries,
                settings.idle_ttl,
                settings.cleanup_interval,
            );
            state.settings_initialized = true;
        }

        if !internal.global.is_empty() && !state.global_rules_initialized {
            for rule in &internal.global {
                state.limiter.add_rule(
                    rule.key.clone(),
                    RateLimitScope::Global,
                    rule.to_runtime_rule(),
                );
            }

            state.global_rules_initialized = true;
        }

        if let Some(scope) = matched_route_scope
            && !internal.matched_route.is_empty()
            && state.initialized_route_scopes.insert(scope)
        {
            for rule in &internal.matched_route {
                state.limiter.add_rule(
                    rule.key.clone(),
                    RateLimitScope::Route(scope),
                    rule.to_runtime_rule(),
                );
            }
        }

        Ok(())
    }

    pub async fn check(
        &self,
        kind: RateLimitKeyKind,
        scope: RateLimitScope,
        value: &str,
    ) -> Result<bool, String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        Ok(state.limiter.check(kind, scope, value))
    }

    pub async fn check_all(
        &self,
        checks: &[(RateLimitKeyKind, RateLimitScope, String)],
    ) -> Result<bool, String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;

        for (kind, scope, value) in checks {
            if !state.limiter.check(kind.clone(), scope.clone(), value) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub async fn check_all_with_results(
        &self,
        checks: &[(RateLimitKeyKind, RateLimitScope, String)],
    ) -> Result<RateLimitCheckResults, String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        Ok(state.limiter.check_with_results(checks))
    }

    pub async fn cleanup_all(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        state.limiter.cleanup();
        Ok(())
    }

    pub async fn stats(&self) -> Result<RateLimiterStats, String> {
        let state = self.state.lock().map_err(|e| e.to_string())?;

        Ok(RateLimiterStats {
            bucket_count: state.limiter.bucket_count(),
            rule_count: state.limiter.rule_count(),
            initialized_route_scope_count: state.initialized_route_scopes.len(),
            global_rules_initialized: state.global_rules_initialized,
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

/// Context key under which `RateLimitManager` is registered in
/// `HttpwardMiddlewareContext::services` during every request.
///
/// Use `httpward_rate_limit_module::get_manager_from_context` to retrieve a
/// typed `Arc<RateLimitManager>` — do not downcast the raw service directly
/// from another DLL binary.
pub const SERVICE_KEY: &str = "httpward_rate_limit::manager";

static RATE_LIMIT_MANAGER: OnceLock<Arc<RateLimitManager>> = OnceLock::new();

/// Initialise (once) and return a clone of the global `Arc<RateLimitManager>`.
///
/// Cheap to call repeatedly — `OnceLock` guarantees single initialisation and
/// `Arc::clone` is just an atomic increment.
pub fn init_global_manager() -> Arc<RateLimitManager> {
    RATE_LIMIT_MANAGER
        .get_or_init(|| Arc::new(RateLimitManager::new()))
        .clone()
}

/// Return a clone of the global manager if it has already been initialised.
pub fn get_global_manager() -> Option<Arc<RateLimitManager>> {
    RATE_LIMIT_MANAGER.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RateLimitStrategy;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn test_manager_initializes_once() {
        let manager = RateLimitManager::new();

        manager
            .init_from_config_sync(
                None,
                &crate::core::HttpWardRateLimitConfig {
                    global_config: Some(crate::core::RateLimitStoreConfig {
                        max_entries: Some(1_000),
                        idle_ttl_sec: Some(60),
                        cleanup_interval_sec: Some(10),
                    }),
                    global_rules: vec![],
                    current_site_rules: vec![],
                    response: None,
                },
            )
            .unwrap();

        manager
            .init_from_config_sync(
                None,
                &crate::core::HttpWardRateLimitConfig {
                    global_config: Some(crate::core::RateLimitStoreConfig {
                        max_entries: Some(5),
                        idle_ttl_sec: Some(1),
                        cleanup_interval_sec: Some(1),
                    }),
                    global_rules: vec![],
                    current_site_rules: vec![],
                    response: None,
                },
            )
            .unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let stats = rt.block_on(manager.stats()).unwrap();
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
            .init_from_config(Some(route_key), &config)
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

        assert!(manager.check_all(&checks).await.unwrap());
        assert!(!manager.check_all(&checks).await.unwrap());
    }

    #[test]
    fn test_manager_sync_init_is_thread_safe() {
        let manager = Arc::new(RateLimitManager::new());
        let barrier = Arc::new(Barrier::new(8));
        let mut handles = Vec::new();

        for _ in 0..8 {
            let manager = Arc::clone(&manager);
            let barrier = Arc::clone(&barrier);

            handles.push(thread::spawn(move || {
                let config = crate::core::HttpWardRateLimitConfig {
                    global_config: Some(crate::core::RateLimitStoreConfig {
                        max_entries: Some(1_000),
                        idle_ttl_sec: Some(60),
                        cleanup_interval_sec: Some(10),
                    }),
                    global_rules: vec![],
                    current_site_rules: vec![],
                    response: None,
                };

                barrier.wait();
                manager.init_from_config_sync(None, &config)
            }));
        }

        for handle in handles {
            handle.join().unwrap().unwrap();
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_manager_sync_init_inside_tokio_runtime() {
        let manager = Arc::new(RateLimitManager::new());

        let join = tokio::spawn({
            let manager = Arc::clone(&manager);
            async move {
                let config = crate::core::HttpWardRateLimitConfig {
                    global_config: Some(crate::core::RateLimitStoreConfig {
                        max_entries: Some(1_000),
                        idle_ttl_sec: Some(60),
                        cleanup_interval_sec: Some(10),
                    }),
                    global_rules: vec![],
                    current_site_rules: vec![],
                    response: None,
                };

                manager.init_from_config_sync(None, &config)
            }
        });

        assert!(join.await.unwrap().is_ok());
    }
}
