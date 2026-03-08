// File: httpward-core/src/httpward_middleware/next.rs

use crate::httpward_middleware::types::{BoxError, BoxService};
use crate::httpward_middleware::middleware_trait::DynMiddleware;
use rama::http::{Body, Request, Response};
use rama::Context;

/// Execution cursor for running middleware chain.
/// Holds a reference to the slice of remaining middlewares and the inner service.
pub struct Next<'a> {
    middlewares: &'a [DynMiddleware],
    inner: &'a BoxService,
}

impl<'a> Next<'a> {
    /// Create a new Next from middlewares slice and boxed inner service.
    pub fn new(middlewares: &'a [DynMiddleware], inner: &'a BoxService) -> Self {
        Self { middlewares, inner }
    }

    /// Run the chain: if there is a middleware left — call it, otherwise call inner service.
    pub async fn run(self, ctx: Context<()>, req: Request<Body>) -> Result<Response<Body>, BoxError> {
        if let Some((first, rest)) = self.middlewares.split_first() {
            // Build the next for the remainder
            let next = Next {
                middlewares: rest,
                inner: self.inner,
            };
            
            // Log middleware call
            if let Some(name) = first.name() {
                tracing::debug!(target: "httpward_middleware", "Calling middleware: {} ({} remaining)", name, rest.len());
            } else {
                tracing::debug!(target: "httpward_middleware", "Calling unnamed middleware ({} remaining)", rest.len());
            }
            
            first.handle(ctx, req, next).await
        } else {
            // No middleware left: call the inner Rama service
            // Call the boxed function directly
            tracing::debug!(target: "httpward_middleware", "No middleware left, calling inner service");
            (self.inner)(ctx, req).await
        }
    }
}
