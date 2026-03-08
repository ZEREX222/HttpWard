// File: httpward-core/src/httpward_middleware/pipe.rs

use std::sync::Arc;
use crate::httpward_middleware::middleware_trait::HttpWardMiddleware;
use crate::httpward_middleware::next::Next;
use crate::httpward_middleware::types::BoxError;
use rama::http::{Body, Request, Response};
use rama::Context;
use rama::service::Service;
use std::fmt;

/// Type alias for boxed middleware stored in the internal Vec.
/// Each middleware must be Send + Sync because the Vec will be shared between threads.
type BoxedMiddleware = Arc<dyn HttpWardMiddleware>;

/// Public wrapper around shared pipeline storage.
/// The internal Vec is wrapped in an Arc so cloning the pipe is cheap (one atomic increment).
#[derive(Clone)]
pub struct HttpWardMiddlewarePipe {
    inner: Arc<Vec<BoxedMiddleware>>,
}

impl Default for HttpWardMiddlewarePipe {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for HttpWardMiddlewarePipe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpWardMiddlewarePipe")
            .field("middleware_count", &self.inner.len())
            .finish()
    }
}

impl HttpWardMiddlewarePipe {
    /// Create an empty, cheap-to-clone pipe.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Vec::new()),
        }
    }

    /// Number of middlewares in the pipe.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Add a middleware to the pipe (returns a new pipe for builder pattern compatibility)
    pub fn add_layer<M>(&self, mw: M) -> Self
    where
        M: HttpWardMiddleware + Send + Sync + 'static,
    {
        let mut new_vec = (*self.inner).clone();
        new_vec.push(Arc::new(mw));
        Self {
            inner: Arc::new(new_vec),
        }
    }

    /// Find layer by name (middleware may return a name via `name()`).
    /// Returns a reference to the boxed middleware if found.
    pub fn get_layer_by_name(
        &self,
        name: &str,
    ) -> Option<&BoxedMiddleware> {
        self.inner.iter().find(|m| m.name().map_or(false, |n| n == name))
    }

    /// Execute the middleware chain for a concrete inner service `S`.
    ///
    /// This converts the concrete `inner` service to a boxed type (`BoxService`) once,
    /// borrows a slice from the internal Arc<Vec<...>> and runs the optimized `Next`.
    /// Hot path: no atomic ops per middleware.
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
        let slice: &[BoxedMiddleware] = &*self.inner;

        // Convert the concrete service to BoxService
        let boxed_service = crate::httpward_middleware::adapter::box_service_from(inner);

        let next = Next::new(slice, &boxed_service);

        next.run(ctx, req).await
    }
}

/// Builder used during configuration time to accumulate Box<dyn Middleware>.
/// Use this builder to add layers; then call `build()` to obtain the cheap-cloneable pipe.
pub struct HttpWardMiddlewarePipeBuilder {
    v: Vec<BoxedMiddleware>,
}

impl HttpWardMiddlewarePipeBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self { v: Vec::new() }
    }

    /// Add a middleware instance (consumes the middleware).
    /// Chaining is supported.
    pub fn add_layer<M>(mut self, mw: M) -> Self
    where
        M: HttpWardMiddleware + Send + Sync + 'static,
    {
        self.v.push(Arc::new(mw));
        self
    }

    /// Add a pre-boxed middleware (useful for plugins that already produce BoxedMiddleware).
    pub fn push_box(mut self, mw: BoxedMiddleware) -> Self {
        self.v.push(mw);
        self
    }

    /// Finalize builder into a cheap-cloneable `HttpWardMiddlewarePipe`.
    pub fn build(self) -> HttpWardMiddlewarePipe {
        HttpWardMiddlewarePipe {
            inner: Arc::new(self.v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rama::http::{Body, Request, Response};
    use rama::Context;
    use crate::httpward_middleware::types::BoxError;
    use crate::httpward_middleware::next::Next;

    // Minimal test middleware to verify plumbing.
    struct DummyMw;
    #[async_trait]
    impl HttpWardMiddleware for DummyMw {
        async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
            // call next without changes
            next.run(ctx, req).await
        }
    }

    #[tokio::test]
    async fn build_and_execute_pipe() {
        // This test ensures builder + pipe compile — not a full integration.
        let builder = HttpWardMiddlewarePipeBuilder::new()
            .add_layer(DummyMw);
        let pipe = builder.build();

        // Can't easily run a full Rama service here — just check metadata.
        assert_eq!(pipe.len(), 1);
        assert!(!pipe.is_empty());
    }
}
