// File: httpward-core/src/httpward_middleware/middleware_trait.rs

use async_trait::async_trait;
use std::sync::Arc;
use crate::httpward_middleware::types::BoxError;
use rama::http::{Body, Request, Response};
use rama::Context;
use crate::httpward_middleware::next::Next;
use crate::core::server_models::site_manager::RouteWithStrategy;

/// HttpWard middleware trait — object-safe, async.
#[async_trait]
pub trait HttpWardMiddleware: Send + Sync + 'static {
    /// Handle a request or call the next middleware.
    ///
    /// # Parameters
    /// - `route_with_strategy`: the matched route together with its resolved active strategy.
    ///
    /// # Notes
    /// - This method is object-safe because it does not use generics.
    /// - The `next` value owns or references the remaining chain and the inner `BoxService`.
    async fn handle(
        &self,
        ctx: Context<()>,
        req: Request<Body>,
        route_with_strategy: Arc<RouteWithStrategy>,
        next: Next<'_>,
    ) -> Result<Response<Body>, BoxError>;

    /// Optional: name for the middleware (useful for get_layer_by_name)
    fn name(&self) -> Option<&'static str> {
        None
    }
    
    /// Dependencies on other middleware (must be present earlier in pipe)
    fn dependencies(&self) -> Vec<&'static str> {
        Vec::new()
    }
}

pub type DynMiddleware = Arc<dyn HttpWardMiddleware>;
