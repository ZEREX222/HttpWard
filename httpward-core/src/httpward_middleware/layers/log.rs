// File: httpward-core/src/httpward_middleware/layers/log.rs

use async_trait::async_trait;
use std::fmt::Debug;
use crate::httpward_middleware::middleware_trait::HttpWardMiddleware;
use crate::httpward_middleware::types::BoxError;
use rama::http::{Body, Request, Response};
use rama::Context;
use crate::httpward_middleware::HttpWardError;
use crate::httpward_middleware::next::Next;
use crate::core::HttpWardContext;

/// Simple logging middleware used as an example.
#[derive(Clone, Debug, Default)]
pub struct HttpWardLogLayer {
    pub tag: Option<String>,
}

impl HttpWardLogLayer {
    pub fn new() -> Self {
        Self { tag: None }
    }

    pub fn with_tag(mut self, t: impl Into<String>) -> Self {
        self.tag = Some(t.into());
        self
    }
}

#[async_trait]
impl HttpWardMiddleware for HttpWardLogLayer {
    async fn handle(
        &self,
        ctx: Context<()>,
        req: Request<Body>,
        next: Next<'_>,
    ) -> Result<Response<Body>, BoxError> {
        //return Err(Box::new(HttpWardError::auth_failed("Invalid token")));
        tracing::debug!(target: "httpward_log", "HttpWardLogLayer.handle called");
        
        // Log incoming request line
        let uri = req.uri().to_string();
        if let Some(tag) = &self.tag {
            tracing::info!(target: "httpward_log", %uri, tag = %tag, "incoming request");
        } else {
            tracing::info!(target: "httpward_log", %uri, "incoming request");
        }

        // Log HttpWardContext information
        if let Some(httpward_ctx) = ctx.get::<HttpWardContext>() {
            // Log current site information
            if let Some(current_site) = &httpward_ctx.current_site {
                tracing::info!(target: "httpward_log", 
                    site_name = %current_site.site_name(),
                    "current site detected"
                );
            } else {
                tracing::info!(target: "httpward_log", "no current site set");
            }

            // Log route information
            let path = req.uri().path();
            match httpward_ctx.get_route(path) {
                Ok(Some(matched_route)) => {
                    tracing::info!(target: "httpward_log",
                        route_type = ?matched_route.route,
                        matcher_type = ?matched_route.matcher_type,
                        strategy_name = %matched_route.active_strategy.name,
                        middleware_count = %matched_route.active_strategy.middleware.len(),
                        params_count = %matched_route.params.len(),
                        params = ?matched_route.params,
                        "route matched with strategy"
                    );
                    
                    // Log detailed middleware information
                    for (i, middleware) in matched_route.active_strategy.middleware.iter().enumerate() {
                        tracing::debug!(target: "httpward_log",
                            middleware_index = i,
                            middleware_config = ?middleware,
                            "middleware detail"
                        );
                    }
                }
                Ok(None) => {
                    tracing::info!(target: "httpward_log", path = %path, "no route matched");
                }
                Err(e) => {
                    tracing::error!(target: "httpward_log", path = %path, error = %e, "error getting route");
                }
            }
        } else {
            tracing::warn!(target: "httpward_log", "HttpWardContext not found in request context");
        }

        // Call next middleware / inner service
        let res = next.run(ctx, req).await?;

        // Log response status (if possible)
        tracing::info!(target: "httpward_log", status = %res.status(), "response produced");

        Ok(res)
    }

    fn name(&self) -> Option<&'static str> {
        // static name for lookup; return None if you prefer
        Some("HttpWardLogLayer")
    }
}
