//! HttpWard Layer Trait and Implementations

use std::any::Any;
use std::fmt::Debug;
use rama::{
    http::{Body, Request, Response},
    layer::Layer,
    service::Service,
    Context,
};

/// Abstract trait for all HttpWard middleware layers
/// 
/// This trait provides a common interface for all middleware components
/// in the HttpWard system, allowing them to be stored and executed
/// through a unified pipeline.
pub trait HttpWardLayer: Any + Send + Sync + Debug {
    /// Get the name of this layer
    fn name(&self) -> &'static str;
    
    /// Clone the layer as a boxed trait object
    fn clone_box(&self) -> Box<dyn HttpWardLayer>;
    
    /// Get the layer as Any for downcasting
    fn as_any(&self) -> &dyn Any;
}

impl Clone for Box<dyn HttpWardLayer> {
    fn clone(&self) -> Box<dyn HttpWardLayer> {
        self.clone_box()
    }
}

/// A logging middleware layer implementation
#[derive(Debug, Clone)]
pub struct HttpWardLogLayer {
    name: String,
}

impl HttpWardLogLayer {
    /// Create a new logging layer
    pub fn new() -> Self {
        Self {
            name: "HttpWardLogLayer".to_string(),
        }
    }
    
    /// Create a logging layer with custom name
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
        }
    }
}

impl Default for HttpWardLogLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpWardLayer for HttpWardLogLayer {
    fn name(&self) -> &'static str {
        "HttpWardLogLayer"
    }
    
    fn clone_box(&self) -> Box<dyn HttpWardLayer> {
        Box::new(self.clone())
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Implement Rama's Layer trait for compatibility
impl<S> Layer<S> for HttpWardLogLayer
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Send + Sync + 'static,
{
    type Service = HttpWardLogService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpWardLogService::new(inner, self.name.clone())
    }
}

/// Service implementation for the logging layer
#[derive(Debug, Clone)]
pub struct HttpWardLogService<S> {
    inner: S,
    name: String,
}

impl<S> HttpWardLogService<S> {
    pub fn new(inner: S, name: String) -> Self {
        Self { inner, name }
    }
}

impl<S> Service<(), Request<Body>> for HttpWardLogService<S>
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Send + Sync + 'static,
    S::Error: Debug + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;

    async fn serve(
        &self,
        ctx: Context<()>,
        request: Request<Body>,
    ) -> Result<Self::Response, Self::Error> {
        // Log the request
        println!("[{}] Processing request: {} {}", self.name, request.method(), request.uri());
        
        // Pass to inner service
        let response = self.inner.serve(ctx, request).await?;
        
        // Log the response
        println!("[{}] Response status: {}", self.name, response.status());
        
        Ok(response)
    }
}
