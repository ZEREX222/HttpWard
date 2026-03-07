//! HttpWard Middleware Pipe
//!
//! Provides a unified pipeline for executing multiple middleware layers.

use std::fmt::Debug;
use rama::{
    http::{Body, Request, Response},
    service::Service,
    Context,
};
use super::layer::HttpWardLayer;

/// A pipeline for managing and executing HttpWard middleware layers
#[derive(Debug, Clone)]
pub struct HttpWardMiddlewarePipe {
    layers: Vec<Box<dyn HttpWardLayer>>,
}

impl HttpWardMiddlewarePipe {
    /// Create a new empty middleware pipe
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
        }
    }
    
    /// Add a layer to the pipe
    pub fn add_layer<T>(mut self, layer: T) -> Self 
    where
        T: HttpWardLayer + 'static,
    {
        self.layers.push(Box::new(layer));
        self
    }
    
    /// Get the number of layers in the pipe
    pub fn len(&self) -> usize {
        self.layers.len()
    }
    
    /// Check if the pipe is empty
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
    
    /// Get a layer by name
    pub fn get_layer_by_name(&self, name: &str) -> Option<&dyn HttpWardLayer> {
        self.layers.iter().find(|layer| layer.name() == name).map(|layer| layer.as_ref())
    }
    
    /// Execute middleware pipeline
    /// 
    /// This method executes all layers in the pipe in sequence.
    /// Currently implements basic execution with the first layer.
    pub async fn execute_middleware<S>(
        &self,
        service: S,
        ctx: Context<()>,
        request: Request<Body>,
    ) -> Result<Response<Body>, S::Error>
    where
        S: Service<(), Request<Body>, Response = Response<Body>> + Clone + Send + Sync + 'static,
        S::Error: Debug + Send + Sync + 'static,
    {
        if let Some(layer) = self.layers.first() {
            // For now, just execute with the first layer
            // In a full implementation, this would chain through all layers
            if let Some(log_layer) = layer.as_any().downcast_ref::<super::layer::HttpWardLogLayer>() {
                // Use the log layer directly
                let log_service = super::layer::HttpWardLogService::new(service.clone(), log_layer.name().to_string());
                return log_service.serve(ctx, request).await;
            }
        }
        
        // Fallback: just call the service directly
        service.serve(ctx, request).await
    }
}

impl Default for HttpWardMiddlewarePipe {
    fn default() -> Self {
        Self::new()
    }
}
