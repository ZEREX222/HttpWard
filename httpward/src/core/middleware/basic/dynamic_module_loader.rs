use std::error::Error;
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
use httpward_core::httpward_middleware::layers::log::HttpWardLogLayer;

/// Layer that dynamically loads and applies HttpWard middleware modules
/// This layer integrates the abstract middleware pipe with Rama's layer system
#[derive(Debug)]
pub struct DynamicModuleLoaderLayer {
    middleware_pipe: HttpWardMiddlewarePipe,
}

impl DynamicModuleLoaderLayer {
    pub fn new() -> Self {
        // Start with empty pipe - layers will be added manually
        let pipe = HttpWardMiddlewarePipe::new()
            .add_layer(HttpWardLogLayer::new().with_tag("dynamic"))
            .add_layer(HttpWardLogLayer::new().with_tag("dynamic1"))
            .add_layer(HttpWardLogLayer::new().with_tag("dynamic2"));

        Self {
            middleware_pipe: pipe,
        }
    }
    
    /// Create a new loader with custom middleware pipe
    pub fn with_pipe(pipe: HttpWardMiddlewarePipe) -> Self {
        Self {
            middleware_pipe: pipe,
        }
    }

    
    /// Dynamically add any layer type to the pipe
    pub fn add_layer<T>(mut self, layer: T) -> Self 
    where
        T: httpward_core::httpward_middleware::HttpWardMiddleware + 'static,
    {
        self.middleware_pipe = self.middleware_pipe.add_layer(layer);
        self
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
    type Error = std::convert::Infallible;

    async fn serve(
        &self,
        ctx: Context<()>,
        request: Request<Body>,
    ) -> Result<Self::Response, Self::Error> {
        tracing::debug!(target: "dynamic_module_loader", "DynamicModuleLoaderService.serve called");
        
        // Execute all middleware in the pipe automatically
        // The pipe handles all the nested middleware logic
        match self.middleware_pipe.execute_middleware(self.inner.clone(), ctx, request).await {
            Ok(response) => {
                tracing::debug!(target: "dynamic_module_loader", "Middleware chain completed successfully");
                Ok(response)
            },
            Err(e) => {
                // Log the error and convert to Infallible
                tracing::error!(target: "dynamic_module_loader", "Middleware error: {:?}", e);
                // For now, we'll return a 500 Internal Server Error response
                // instead of trying to convert to Infallible
                Ok(Response::builder()
                    .status(500)
                    .body("Internal Server Error".into())
                    .unwrap())
            }
        }
    }
}
