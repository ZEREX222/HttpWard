// File: httpward-core/src/httpward_middleware/middleware_trait.rs

use async_trait::async_trait;
use std::sync::Arc;
use crate::httpward_middleware::types::BoxError;
use rama::http::{Body, Request, Response};
use rama::Context;
use crate::httpward_middleware::next::Next;

/// HttpWard middleware trait — object-safe, async.
#[async_trait]
pub trait HttpWardMiddleware: Send + Sync + 'static {
    /// Handle a request or call the next middleware.
    ///
    /// # Notes
    /// - This method is object-safe because it does not use generics.
    /// - The `next` value owns or references the remaining chain and the inner `BoxService`.
    async fn handle(
        &self,
        ctx: Context<()>,
        req: Request<Body>,
        next: Next<'_>,
    ) -> Result<Response<Body>, BoxError>;

    /// Optional: name for the middleware (useful for get_layer_by_name)
    fn name(&self) -> Option<&'static str> {
        None
    }
}

pub type DynMiddleware = Arc<dyn HttpWardMiddleware>;
