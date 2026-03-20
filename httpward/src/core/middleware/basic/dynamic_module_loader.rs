use std::sync::Arc;
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
    fn build_route_pipes(&self, server_instance: &Arc<ServerInstance>) -> HashMap<usize, HttpWardMiddlewarePipe> {
        let mut route_pipes = HashMap::new();

        for site_manager in &server_instance.site_managers {
            for route_with_strategy in site_manager.routes_with_strategy() {
                // Collect names of ONLY the enabled middleware for this specific route
                let active_names: HashSet<&str> = route_with_strategy
                    .active_strategy
                    .middleware
                    .iter()
                    .filter_map(|mc| match mc {
                        httpward_core::config::strategy::MiddlewareConfig::Named { name, .. }
                        | httpward_core::config::strategy::MiddlewareConfig::On { name } => Some(name.as_str()),
                        httpward_core::config::strategy::MiddlewareConfig::Off { .. } => None,
                    })
                    .collect();

                // Create a filtered pipe — cheap (Arc<dyn Middleware> clones only)
                let filtered_pipe = self.middleware_pipe.create_filtered(&active_names);

                // Stable pointer to Arc<Route> as the lookup key
                let key = Arc::as_ptr(&route_with_strategy.route) as usize;

                tracing::debug!(
                    target: "dynamic_module_loader",
                    "Route {:?}: precomputed pipe with {}/{} middleware (active: {:?})",
                    route_with_strategy.route.get_match(),
                    filtered_pipe.len(),
                    self.middleware_pipe.len(),
                    active_names,
                );

                route_pipes.insert(key, filtered_pipe);
            }
        }

        route_pipes
    }

    /// Collect unique middleware names from all strategies across all site managers
    fn collect_unique_middleware_names(&self, server_instance: &Arc<ServerInstance>) -> Vec<String> {
        let mut middleware_names = HashSet::new();

        for site_manager in &server_instance.site_managers {
            for route_with_strategy in site_manager.routes_with_strategy() {
                for middleware_config in route_with_strategy.active_strategy.middleware.iter() {
                    // Only include enabled middleware (skip Off middleware)
                    match middleware_config {
                        httpward_core::config::strategy::MiddlewareConfig::Named { name, .. }
                        | httpward_core::config::strategy::MiddlewareConfig::On { name } => {
                            middleware_names.insert(name.clone());
                        }
                        httpward_core::config::strategy::MiddlewareConfig::Off { .. } => {
                            // Skip disabled middleware
                        }
                    }
                }
            }
        }

        let mut names: Vec<String> = middleware_names.into_iter().collect();
        names.sort(); // Sort for consistent ordering
        names
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
    /// Lookup order:
    /// 1. `HttpWardContext` present in `ctx` → find `current_site` → call `get_route(path)` →
    ///    look up `Arc::as_ptr(&matched.route)` in `route_pipes`
    /// 2. Fallback to the global `middleware_pipe`.
    fn resolve_pipe<'a>(&'a self, ctx: &Context<()>, req: &Request<Body>) -> &'a HttpWardMiddlewarePipe {
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
                        return pipe;
                    }
                }
            }
        }
        tracing::debug!(
            target: "dynamic_module_loader",
            "Using global fallback pipe ({} middleware)",
            self.middleware_pipe.len(),
        );
        &self.middleware_pipe
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
        ctx: Context<()>,
        request: Request<Body>,
    ) -> Result<Self::Response, Self::Error> {
        tracing::debug!(target: "dynamic_module_loader", "DynamicModuleLoaderService.serve called");

        // Pick the precomputed per-route filtered pipe, or fall back to the global one.
        // This is an O(1) HashMap lookup — no allocation, no filtering at request time.
        let pipe = self.resolve_pipe(&ctx, &request);

        pipe.execute_middleware(self.inner.clone(), ctx, request).await
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
