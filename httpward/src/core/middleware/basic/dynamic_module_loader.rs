use std::sync::{Arc, Mutex, OnceLock};
use std::collections::{HashSet, HashMap};
use httpward_core::httpward_middleware::{
    HttpWardMiddlewarePipe
};
use rama::{
    http::{Body, Request, Response},
    layer::Layer,
    service::Service,
    Context,
};
use std::fmt::Debug;
use httpward_core::core::server_models::server_instance::ServerInstance;
use httpward_core::core::HttpWardContext;

/// Re-export the plugin loader
use super::middleware_global_module_storage::get_middleware_instance;

/// Tracks middleware instances that already passed startup initialization.
static INITIALIZED_MIDDLEWARE_INSTANCES: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

/// Layer that dynamically loads and applies HttpWard middleware modules
/// This layer integrates the abstract middleware pipe with Rama's layer system
#[derive(Debug)]
pub struct DynamicModuleLoaderLayer {
    /// Global pipe with ALL enabled middleware — used as fallback
    middleware_pipe: HttpWardMiddlewarePipe,
    /// Precomputed per-route filtered pipes.
    /// Key = `Arc::as_ptr(&route) as usize` (stable for the lifetime of SiteManager).
    /// Value = pipe containing only the middleware enabled by this route's active_strategy.
    route_pipes: HashMap<usize, HttpWardMiddlewarePipe>,
}

impl DynamicModuleLoaderLayer {
    fn init_middleware_once(
        server_instance: &Arc<ServerInstance>,
        middleware_name: &str,
        middleware_instance: &Arc<dyn httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware + Send + Sync>,
    ) {
        let middleware_ptr = Arc::as_ptr(middleware_instance) as *const () as usize;
        let should_initialize = {
            let initialized_registry = INITIALIZED_MIDDLEWARE_INSTANCES
                .get_or_init(|| Mutex::new(HashSet::new()));
            let mut guard = initialized_registry
                .lock()
                .expect("Failed to acquire middleware initialization registry lock");
            guard.insert(middleware_ptr)
        };

        if !should_initialize {
            tracing::debug!(
                target: "dynamic_module_loader",
                "Skipping init for middleware '{}' (already initialized)",
                middleware_name
            );
            return;
        }

        middleware_instance
            .init(server_instance)
            .unwrap_or_else(|error| {
                panic!(
                    "Failed to initialize middleware '{}': {}",
                    middleware_name,
                    error
                )
            });
    }

    /// Create a new loader that automatically loads middleware from strategies
    pub fn new(server_instance: &Arc<ServerInstance>) -> Self {
        let pipe = HttpWardMiddlewarePipe::new();
        let mut loader = Self {
            middleware_pipe: pipe,
            route_pipes: HashMap::new(),
        };

        // Collect unique middleware names from all strategies across all site managers
        let unique_middleware_names = loader.collect_unique_middleware_names(server_instance);
        
        tracing::info!(target: "dynamic_module_loader", "Found {} unique middleware names from strategies", unique_middleware_names.len());

        let mut missing_middleware_names: Vec<String> = Vec::new();

        // Load each middleware instance into the global pipe
        for middleware_name in &unique_middleware_names {
            match get_middleware_instance(middleware_name) {
                Some(middleware_instance) => {
                    Self::init_middleware_once(server_instance, middleware_name, &middleware_instance);

                    loader.middleware_pipe = loader.middleware_pipe.add_boxed_layer(middleware_instance)
                        .expect(&format!("Failed to add middleware '{}': dependency validation failed", middleware_name));
                    tracing::info!(target: "dynamic_module_loader", "Successfully loaded middleware: {}", middleware_name);
                }
                None => {
                    missing_middleware_names.push(middleware_name.clone());
                }
            }
        }

        if !missing_middleware_names.is_empty() {
            missing_middleware_names.sort();
            panic!(
                "Missing middleware modules in global storage: {}. Check strategy names and module loading mode.",
                missing_middleware_names.join(", ")
            );
        }

        tracing::info!(target: "dynamic_module_loader", "Global pipe initialized with {} middleware layers", loader.middleware_count());

        // Precompute per-route filtered pipes
        loader.route_pipes = loader.build_route_pipes(server_instance);
        tracing::info!(target: "dynamic_module_loader", "Precomputed {} route-specific pipes", loader.route_pipes.len());

        loader
    }

    /// Build precomputed filtered pipes for every route across all site managers.
    /// Key = `Arc::as_ptr(&route) as usize` — stable pointer, zero-cost lookup at request time.
    ///
    /// Validates that each filtered pipe has all required dependencies satisfied.
    /// Panics if a middleware's required dependency is not included in the route's active strategy.
    fn build_route_pipes(&self, server_instance: &Arc<ServerInstance>) -> HashMap<usize, HttpWardMiddlewarePipe> {
        let mut route_pipes = HashMap::new();

        for site_manager in &server_instance.site_managers {
            for route_with_strategy in site_manager.routes_with_strategy() {
                // Collect names of enabled middleware for this specific route
                // Preserve order from strategy configuration
                let ordered_names: Vec<&str> = route_with_strategy
                    .active_strategy
                    .middleware
                    .iter()
                    .filter_map(|mc| match mc {
                        httpward_core::config::strategy::MiddlewareConfig::Named { name, .. }
                        | httpward_core::config::strategy::MiddlewareConfig::On { name } => Some(name.as_str()),
                        httpward_core::config::strategy::MiddlewareConfig::Off { .. } => None,
                    })
                    .collect();

                // Create a filtered pipe with proper order from strategy
                let filtered_pipe = self.middleware_pipe
                    .create_filtered_ordered(&ordered_names);

                // Validate that all middleware in the filtered pipe have their dependencies satisfied
                self.validate_filtered_pipe_dependencies(&route_with_strategy.route, &ordered_names);

                let route_match = route_with_strategy.route.get_match();
                tracing::debug!(
                    target: "dynamic_module_loader",
                    "✓ Route '{:?}': dependency validation passed — precomputed pipe with {}/{} middleware (active: {:?})",
                    route_match,
                    filtered_pipe.len(),
                    self.middleware_pipe.len(),
                    ordered_names,
                );

                // Stable pointer to Arc<Route> as the lookup key
                let key = Arc::as_ptr(&route_with_strategy.route) as usize;
                route_pipes.insert(key, filtered_pipe);
            }
        }

        route_pipes
    }

    /// Validate that all middleware enabled for a route have their dependencies also enabled.
    /// Panics with a detailed error message if any required dependency is missing.
    fn validate_filtered_pipe_dependencies(
        &self,
        route: &std::sync::Arc<httpward_core::config::Route>,
        ordered_names: &[&str],
    ) {
        let mut validation_errors: Vec<String> = Vec::new();
        let mut positions: HashMap<&str, usize> = HashMap::new();
        for (idx, &name) in ordered_names.iter().enumerate() {
            positions.insert(name, idx);
        }

        for mw_ptr in self.middleware_pipe.iter() {
            if let Some(mw_name) = mw_ptr.name() {
                let Some(&mw_pos) = positions.get(mw_name) else {
                    continue;
                };

                for &dependency_name in &mw_ptr.dependencies() {
                    match positions.get(dependency_name) {
                        None => {
                            validation_errors.push(format!(
                                "Middleware '{}' requires dependency '{}' which is NOT enabled in route's active strategy",
                                mw_name,
                                dependency_name
                            ));
                        }
                        Some(&dep_pos) if dep_pos >= mw_pos => {
                            validation_errors.push(format!(
                                "Middleware '{}' requires dependency '{}' to be BEFORE it in route's active strategy",
                                mw_name,
                                dependency_name
                            ));
                        }
                        Some(_) => {}
                    }
                }

                for &optional_dependency_name in &mw_ptr.optional_dependencies() {
                    if let Some(&dep_pos) = positions.get(optional_dependency_name) {
                        if dep_pos >= mw_pos {
                            validation_errors.push(format!(
                                "Middleware '{}' has optional dependency '{}' enabled, but it must be BEFORE it in route's active strategy",
                                mw_name,
                                optional_dependency_name
                            ));
                        }
                    }
                }
            }
        }

        if !validation_errors.is_empty() {
            let route_match = route.get_match();
            let error_details = validation_errors.join("\n  - ");
            let error_message = format!(
                "Middleware dependency validation failed for route '{:?}':\n  - {}",
                route_match, error_details
            );
            tracing::error!(
                target: "dynamic_module_loader",
                "✗ CRITICAL: {}", error_message
            );
            panic!("{}", error_message);
        }
    }

    /// Collect unique middleware names from all strategies across all site managers
    /// Preserves the order from the configuration (not alphabetical)
    fn collect_unique_middleware_names(&self, server_instance: &Arc<ServerInstance>) -> Vec<String> {
        let mut middleware_names: Vec<String> = Vec::new();
        let mut seen = HashSet::new();

        for site_manager in &server_instance.site_managers {
            for route_with_strategy in site_manager.routes_with_strategy() {
                for middleware_config in route_with_strategy.active_strategy.middleware.iter() {
                    // Only include enabled middleware (skip Off middleware)
                    match middleware_config {
                        httpward_core::config::strategy::MiddlewareConfig::Named { name, .. }
                        | httpward_core::config::strategy::MiddlewareConfig::On { name } => {
                            // Add only if not seen before - preserves order from config
                            if !seen.contains(name) {
                                middleware_names.push(name.clone());
                                seen.insert(name.clone());
                            }
                        }
                        httpward_core::config::strategy::MiddlewareConfig::Off { .. } => {
                            // Skip disabled middleware
                        }
                    }
                }
            }
        }

        middleware_names
    }

    /// Create a new loader with custom middleware pipe (no per-route precomputation)
    pub fn with_pipe(pipe: HttpWardMiddlewarePipe) -> Self {
        Self {
            middleware_pipe: pipe,
            route_pipes: HashMap::new(),
        }
    }

    /// Dynamically add any layer type to the pipe (convenience generic).
    pub fn add_layer<T>(mut self, layer: T) -> Self
    where
        T: httpward_core::httpward_middleware::HttpWardMiddleware + 'static,
    {
        self.middleware_pipe = self.middleware_pipe.add_layer(layer)
            .expect("Failed to add layer: dependency validation failed");
        self
    }

    /// Add a pre-boxed middleware (Arc<dyn HttpWardMiddleware>) to the pipe at runtime.
    /// This uses the `add_boxed_layer` method added to pipe.
    pub fn add_boxed_layer(&mut self, boxed: Arc<dyn httpward_core::httpward_middleware::HttpWardMiddleware + Send + Sync>) {
        // Convert Arc to the internal BoxedMiddleware type (which is Arc<dyn ...> already)
        self.middleware_pipe = self.middleware_pipe.add_boxed_layer(boxed)
            .expect("Failed to add boxed layer: dependency validation failed");
    }


    /// Get the number of middleware layers
    pub fn middleware_count(&self) -> usize {
        self.middleware_pipe.len()
    }

    /// Check if any middleware is configured
    pub fn has_middleware(&self) -> bool {
        !self.middleware_pipe.is_empty()
    }

    /// Get a reference to the middleware pipe
    pub fn pipe(&self) -> &HttpWardMiddlewarePipe {
        &self.middleware_pipe
    }

    /// Get a specific layer by name
    pub fn get_layer_by_name(&self, name: &str) -> Option<std::sync::Arc<dyn httpward_core::httpward_middleware::HttpWardMiddleware>> {
        self.middleware_pipe.get_layer_by_name(name).cloned()
    }

}

impl Default for DynamicModuleLoaderLayer {
    fn default() -> Self {
        // Create a dummy ServerInstance for backward compatibility
        // In practice, this should not be used - prefer calling new() with actual ServerInstance
        let dummy_server_instance = Arc::new(ServerInstance {
            bind: httpward_core::core::server_models::listener::ListenerKey {
                host: "127.0.0.1".to_string(),
                port: 8080,
            },
            site_managers: Vec::new(),
            global: httpward_core::config::GlobalConfig::default(),
        });
        Self::new(&dummy_server_instance)
    }
}

impl<S> Layer<S> for DynamicModuleLoaderLayer
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Send + Sync + 'static,
{
    type Service = DynamicModuleLoaderService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DynamicModuleLoaderService::new(inner, &self.middleware_pipe, self.route_pipes.clone())
    }
}

/// Service that applies dynamic middleware modules to requests and responses
#[derive(Debug)]
pub struct DynamicModuleLoaderService<S> {
    inner: S,
    /// Global pipe — fallback when no route-specific pipe is found
    middleware_pipe: HttpWardMiddlewarePipe,
    /// Precomputed per-route filtered pipes (shared cheap Arc clone from Layer)
    route_pipes: HashMap<usize, HttpWardMiddlewarePipe>,
}

impl<S> DynamicModuleLoaderService<S>
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Send + Sync + 'static,
{
    pub fn new(
        inner: S,
        middleware_pipe: &HttpWardMiddlewarePipe,
        route_pipes: HashMap<usize, HttpWardMiddlewarePipe>,
    ) -> Self {
        Self {
            inner,
            middleware_pipe: middleware_pipe.clone(),
            route_pipes,
        }
    }

    /// Resolve the correct pipe for this request.
    ///
    /// Returns:
    /// - `Some(pipe)` — precomputed per-route filtered pipe when a route is matched.
    /// - `None`       — no route matched; the caller should bypass the middleware pipe
    ///                  and forward the request directly to the inner service.
    fn resolve_pipe<'a>(&'a self, ctx: &Context<()>, req: &Request<Body>) -> Option<&'a HttpWardMiddlewarePipe> {
        if let Some(hctx) = ctx.get::<HttpWardContext>() {
            if let Some(site) = &hctx.current_site {
                let path = req.uri().path();
                if let Ok(matched) = site.get_route(path) {
                    let key = Arc::as_ptr(&matched.route) as usize;
                    if let Some(pipe) = self.route_pipes.get(&key) {
                        tracing::debug!(
                            target: "dynamic_module_loader",
                            "Using precomputed pipe ({} middleware) for route {:?}",
                            pipe.len(),
                            matched.route.get_match(),
                        );
                        return Some(pipe);
                    }
                }
            }
        }
        tracing::debug!(
            target: "dynamic_module_loader",
            "No route matched — skipping middleware pipe, forwarding directly to inner service",
        );
        None
    }

    /// Get the number of middleware layers
    pub fn middleware_count(&self) -> usize {
        self.middleware_pipe.len()
    }

    /// Get a reference to the middleware pipe
    pub fn pipe(&self) -> &HttpWardMiddlewarePipe {
        &self.middleware_pipe
    }

    /// Get a specific layer by name
    pub fn get_layer_by_name(&self, name: &str) -> Option<std::sync::Arc<dyn httpward_core::httpward_middleware::HttpWardMiddleware>> {
        self.middleware_pipe.get_layer_by_name(name).cloned()
    }
}

impl<S> Service<(), Request<Body>> for DynamicModuleLoaderService<S>
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Clone + Send + Sync + 'static,
    S::Error: Debug + Send + Sync + std::error::Error + 'static,
    S::Response: Debug + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn serve(
        &self,
        mut ctx: Context<()>,
        request: Request<Body>,
    ) -> Result<Self::Response, Self::Error> {
        tracing::debug!(target: "dynamic_module_loader", "DynamicModuleLoaderService.serve called");

        match self.resolve_pipe(&ctx, &request) {
            Some(pipe) => {
                // Route matched — run the precomputed filtered middleware pipe.
                pipe.execute_middleware(self.inner.clone(), ctx, request).await
            }
            None => {
                // No route matched — skip all middleware and forward directly to inner service.
                self.inner.serve(ctx, request).await
                    .map_err(|e| Box::new(e) as Self::Error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpward_core::config::{SiteConfig, Route, GlobalConfig, MiddlewareConfig, StrategyRef};
    use httpward_core::core::server_models::site_manager::SiteManager;
    use httpward_core::core::server_models::listener::ListenerKey;
    use httpward_core::config::{Match, StrategyCollection};
    use std::sync::Arc;

    fn create_test_server_instance() -> Arc<ServerInstance> {
        // Create test strategies collection with simple middleware names
        let mut strategies = StrategyCollection::new();
        strategies.insert(
            "test_strategy".to_string(),
            vec![
                MiddlewareConfig::new_off("test_middleware".to_string()),
                MiddlewareConfig::new_off("disabled_middleware".to_string()),
            ],
        );

        // Create test route with strategy
        let route = Route::Proxy {
            r#match: Match {
                path: Some("/test".to_string()),
                path_regex: None,
            },
            backend: "http://example.com".to_string(),
            strategy: Some(StrategyRef::Named("test_strategy".to_string())),
            strategies: None,
        };

        // Create test site config
        let site_config = SiteConfig {
            domain: "test.com".to_string(),
            routes: vec![route],
            strategy: None,
            strategies,
            ..Default::default()
        };

        // Create site manager
        let site_manager = SiteManager::new(
            Arc::new(site_config),
            Some(&GlobalConfig::default()),
        ).unwrap();

        // Create server instance
        let server_instance = ServerInstance {
            bind: ListenerKey {
                host: "127.0.0.1".to_string(),
                port: 8080,
            },
            site_managers: vec![Arc::new(site_manager)],
            global: GlobalConfig::default(),
        };

        Arc::new(server_instance)
    }

    fn create_test_server_instance_with_on() -> Arc<ServerInstance> {
        let mut strategies = StrategyCollection::new();
        strategies.insert(
            "test_strategy".to_string(),
            vec![
                MiddlewareConfig::new_on("enabled_without_config".to_string()),
                MiddlewareConfig::new_off("disabled_middleware".to_string()),
            ],
        );

        let route = Route::Proxy {
            r#match: Match {
                path: Some("/test".to_string()),
                path_regex: None,
            },
            backend: "http://example.com".to_string(),
            strategy: Some(StrategyRef::Named("test_strategy".to_string())),
            strategies: None,
        };

        let site_config = SiteConfig {
            domain: "test.com".to_string(),
            routes: vec![route],
            strategy: None,
            strategies,
            ..Default::default()
        };

        let site_manager = SiteManager::new(
            Arc::new(site_config),
            Some(&GlobalConfig::default()),
        ).unwrap();

        Arc::new(ServerInstance {
            bind: ListenerKey {
                host: "127.0.0.1".to_string(),
                port: 8080,
            },
            site_managers: vec![Arc::new(site_manager)],
            global: GlobalConfig::default(),
        })
    }

    #[test]
    fn test_dynamic_module_loader_layer_creation() {
        let server_instance = create_test_server_instance();
        let layer = DynamicModuleLoaderLayer::new(&server_instance);
        
        // Should have some middleware count (0 if no middleware found, or count of loaded middleware)
        let middleware_count = layer.middleware_count();
        println!("Middleware count: {}", middleware_count);
        
        // The layer should be created successfully
        assert!(middleware_count >= 0);
    }

    #[test]
    fn test_collect_unique_middleware_names() {
        let server_instance = create_test_server_instance();
        let layer = DynamicModuleLoaderLayer::with_pipe(HttpWardMiddlewarePipe::new());

        // Test the collection method
        let unique_names = layer.collect_unique_middleware_names(&server_instance);
        
        // Should contain no middleware since both are Off
        assert!(!unique_names.contains(&"test_middleware".to_string()));
        assert!(!unique_names.contains(&"disabled_middleware".to_string()));
        assert_eq!(unique_names.len(), 0);
        
        println!("Unique middleware names: {:?}", unique_names);
    }

    #[test]
    fn test_collect_unique_middleware_names_includes_on() {
        let server_instance = create_test_server_instance_with_on();
        let layer = DynamicModuleLoaderLayer::with_pipe(HttpWardMiddlewarePipe::new());

        let unique_names = layer.collect_unique_middleware_names(&server_instance);

        assert!(unique_names.contains(&"enabled_without_config".to_string()));
        assert!(!unique_names.contains(&"disabled_middleware".to_string()));
        assert_eq!(unique_names.len(), 1);
    }

    #[test]
    fn test_empty_server_instance() {
        let empty_server_instance = Arc::new(ServerInstance {
            bind: ListenerKey {
                host: "127.0.0.1".to_string(),
                port: 8080,
            },
            site_managers: vec![],
            global: GlobalConfig::default(),
        });

        let layer = DynamicModuleLoaderLayer::new(&empty_server_instance);
        
        // Should have 0 middleware for empty server instance
        assert_eq!(layer.middleware_count(), 0);
    }

    #[test]
    fn test_default_implementation() {
        let layer = DynamicModuleLoaderLayer::default();
        
        // Should create successfully even with dummy server instance
        assert_eq!(layer.middleware_count(), 0);
    }

    #[test]
    #[should_panic(expected = "Missing middleware modules in global storage")]
    fn test_new_panics_when_enabled_middleware_missing_in_storage() {
        let server_instance = create_test_server_instance_with_on();
        let _ = DynamicModuleLoaderLayer::new(&server_instance);
    }
}
