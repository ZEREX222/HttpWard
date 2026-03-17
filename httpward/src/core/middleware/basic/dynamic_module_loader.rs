use std::sync::Arc;
use std::collections::HashSet;
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
use httpward_core::httpward_middleware::pipe::HttpWardMiddlewarePipeBuilder;
use httpward_core::core::server_models::server_instance::ServerInstance;

/// Re-export the plugin loader
use super::middleware_global_module_storage::get_middleware_instance;

/// Layer that dynamically loads and applies HttpWard middleware modules
/// This layer integrates the abstract middleware pipe with Rama's layer system
#[derive(Debug)]
pub struct DynamicModuleLoaderLayer {
    middleware_pipe: HttpWardMiddlewarePipe,
}

impl DynamicModuleLoaderLayer {
    /// Create a new loader that automatically loads middleware from strategies
    pub fn new(server_instance: &Arc<ServerInstance>) -> Self {
        let pipe = HttpWardMiddlewarePipeBuilder::new().build();
        let mut loader = Self {
            middleware_pipe: pipe,
        };

        // Collect unique middleware names from all strategies across all site managers
        let unique_middleware_names = loader.collect_unique_middleware_names(server_instance);
        
        tracing::info!(target: "dynamic_module_loader", "Found {} unique middleware names from strategies", unique_middleware_names.len());

        // Load each middleware instance
        for middleware_name in &unique_middleware_names {
            match get_middleware_instance(middleware_name) {
                Some(middleware_instance) => {
                    loader.middleware_pipe = loader.middleware_pipe.add_boxed_layer(middleware_instance);
                    tracing::info!(target: "dynamic_module_loader", "Successfully loaded middleware: {}", middleware_name);
                }
                None => {
                    tracing::warn!(target: "dynamic_module_loader", "Failed to load middleware '{}': not found in global storage", middleware_name);
                }
            }
        }

        tracing::info!(target: "dynamic_module_loader", "DynamicModuleLoaderLayer initialized with {} middleware layers", loader.middleware_count());
        loader
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

    /// Create a new loader with custom middleware pipe
    pub fn with_pipe(pipe: HttpWardMiddlewarePipe) -> Self {
        Self {
            middleware_pipe: pipe,
        }
    }

    /// Dynamically add any layer type to the pipe (convenience generic).
    pub fn add_layer<T>(mut self, layer: T) -> Self
    where
        T: httpward_core::httpward_middleware::HttpWardMiddleware + 'static,
    {
        self.middleware_pipe = self.middleware_pipe.add_layer(layer);
        self
    }

    /// Add a pre-boxed middleware (Arc<dyn HttpWardMiddleware>) to the pipe at runtime.
    /// This uses the `add_boxed_layer` method added to pipe.
    pub fn add_boxed_layer(&mut self, boxed: Arc<dyn httpward_core::httpward_middleware::HttpWardMiddleware + Send + Sync>) {
        // Convert Arc to the internal BoxedMiddleware type (which is Arc<dyn ...> already)
        self.middleware_pipe = self.middleware_pipe.add_boxed_layer(boxed);
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
        DynamicModuleLoaderService::new(inner, &self.middleware_pipe)
    }
}

/// Service that applies dynamic middleware modules to requests and responses
#[derive(Debug)]
pub struct DynamicModuleLoaderService<S> {
    inner: S,
    middleware_pipe: HttpWardMiddlewarePipe,
}

impl<S> DynamicModuleLoaderService<S>
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Send + Sync + 'static,
{
    pub fn new(inner: S, middleware_pipe: &HttpWardMiddlewarePipe) -> Self {
        Self {
            inner,
            middleware_pipe: middleware_pipe.clone(), // Use the passed pipe instead of empty one
        }
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

        // Execute all middleware in the pipe automatically
        // The pipe handles all the nested middleware logic
        self.middleware_pipe.execute_middleware(self.inner.clone(), ctx, request).await
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
        let layer = DynamicModuleLoaderLayer::new(&server_instance);
        
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
        let layer = DynamicModuleLoaderLayer::new(&server_instance);

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
}
