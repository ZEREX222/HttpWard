// File: httpward-core/src/httpward_middleware/layers/log.rs

use async_trait::async_trait;
use std::fmt::Debug;
use crate::httpward_middleware::middleware_trait::HttpWardMiddleware;
use crate::httpward_middleware::types::BoxError;
use rama::http::{Body, Request, Response};
use rama::Context;
use crate::httpward_middleware::next::Next;

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
        tracing::debug!(target: "httpward_log", "HttpWardLogLayer.handle called");
        
        // Log incoming request line
        let uri = req.uri().to_string();
        if let Some(tag) = &self.tag {
            tracing::info!(target: "httpward_log", %uri, tag = %tag, "incoming request");
        } else {
            tracing::info!(target: "httpward_log", %uri, "incoming request");
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
