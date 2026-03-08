// File: httpward-core/src/httpward_middleware/pipe.rs

use std::sync::Arc;
use crate::httpward_middleware::middleware_trait::DynMiddleware;
use crate::httpward_middleware::next::Next;
use crate::httpward_middleware::types::BoxError;
use rama::http::{Body, Request, Response};
use rama::Context;
use crate::httpward_middleware::adapter::box_service_from;
use rama::service::Service;

/// Runtime middleware pipe: holds Vec<Arc<dyn HttpWardMiddleware>> and executes them.
#[derive(Clone, Default)]
pub struct HttpWardMiddlewarePipe {
    middlewares: Vec<DynMiddleware>,
}

impl std::fmt::Debug for HttpWardMiddlewarePipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpWardMiddlewarePipe")
            .field("middleware_count", &self.middlewares.len())
            .finish()
    }
}

impl HttpWardMiddlewarePipe {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    /// Add a middleware to the end of the chain.
    pub fn add_layer<M>(mut self, mw: M) -> Self
    where
        M: crate::httpward_middleware::middleware_trait::HttpWardMiddleware + 'static,
    {
        self.middlewares.push(Arc::new(mw));
        self
    }

    /// Add middleware by Arc (useful for prebuilt components / plugins)
    pub fn push_arc(&mut self, mw: DynMiddleware) {
        self.middlewares.push(mw);
    }

    pub fn len(&self) -> usize {
        self.middlewares.len()
    }

    pub fn is_empty(&self) -> bool {
        self.middlewares.is_empty()
    }

    /// Find layer by name (middleware may return a name via `name()`).
    pub fn get_layer_by_name(&self, name: &str) -> Option<&DynMiddleware> {
        self.middlewares.iter().find(|m| m.name().map_or(false, |n| n == name))
    }

    /// Execute the middleware chain for a concrete inner service `S`.
    ///
    /// NOTE: The returned error type is BoxError. To integrate with your existing
    /// DynamicModuleLoaderService you can change its `Error` to BoxError or map it.
    pub async fn execute_middleware<S>(
        &self,
        inner: S,
        ctx: Context<()>,
        req: Request<Body>,
    ) -> Result<Response<Body>, BoxError>
    where
        S: Service<(), Request<Body>, Response = Response<Body>> + Clone + Send + Sync + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        tracing::debug!(target: "httpward_middleware", "execute_middleware called with {} middlewares", self.middlewares.len());
        
        // Convert concrete service to BoxService
        let boxed = box_service_from(inner);

        // Build Next and run
        let next = Next::new(&self.middlewares, &boxed);
        let result = next.run(ctx, req).await;
        
        tracing::debug!(target: "httpward_middleware", "execute_middleware completed with result: {:?}", 
            result.as_ref().map(|r| r.status().as_u16()));
        
        result
    }
}
