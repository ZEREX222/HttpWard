use std::sync::Arc;
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

/// Re-export the plugin loader
use super::middleware_global_module_storage::get_middleware_instance;

/// Layer that dynamically loads and applies HttpWard middleware modules
/// This layer integrates the abstract middleware pipe with Rama's layer system
#[derive(Debug)]
pub struct DynamicModuleLoaderLayer {
    middleware_pipe: HttpWardMiddlewarePipe,
}

impl DynamicModuleLoaderLayer {
    pub fn new() -> Self {
        let pipe = HttpWardMiddlewarePipeBuilder::new().build();
        let mut loader = Self {
            middleware_pipe: pipe,
        };

        // Load log middleware directly from global storage
        match get_middleware_instance("httpward_log_module") {
            Some(middleware_instance) => {
                loader.middleware_pipe = loader.middleware_pipe.add_boxed_layer(middleware_instance);
                tracing::info!(target: "dynamic_module_loader", "Successfully loaded log middleware");
            }
            None => {
                tracing::error!(target: "dynamic_module_loader", "Failed to load log middleware: not found in global storage");
            }
        }

        tracing::info!(target: "dynamic_module_loader", "DynamicModuleLoaderLayer initialized with {} middleware layers", loader.middleware_count());
        loader
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
        Self::new()
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
