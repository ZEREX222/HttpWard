// File: httpward-modules/httpward_log_module/src/httpward_log_layer

use async_trait::async_trait;
use std::fmt::Debug;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::types::BoxError;
use httpward_core::httpward_middleware::next::Next;
use rama::http::{Body, Request, Response};
use rama::Context;
use httpward_core::core::HttpWardContext;
use httpward_core::module_logging::ModuleLogger;

// Import the custom logging macros
use httpward_core::{module_log_error, module_log_warn, module_log_info, module_log_debug};

/// Simple logging middleware for HttpWard with custom module logging.
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
        module_log_debug!("HttpWardLogLayer.handle called");
        
        // Log incoming request line
        let uri = req.uri().to_string();
        if let Some(tag) = &self.tag {
            module_log_info!("incoming request - URI: {}, tag: {}", uri, tag);
        } else {
            module_log_info!("incoming request - URI: {}", uri);
        }

        // Log HttpWardContext information
        if let Some(httpward_ctx) = ctx.get::<HttpWardContext>() {
            // Log current site information
            if let Some(current_site) = &httpward_ctx.current_site {
                module_log_info!("current site detected - site_name: {}", current_site.site_name());
            } else {
                module_log_info!("no current site set");
            }

            // Log route information
            let path = req.uri().path();
            match httpward_ctx.get_route(path) {
                Ok(Some(matched_route)) => {
                    module_log_info!("route matched with strategy - route_type: {:?}, matcher_type: {:?}, strategy_name: {}, middleware_count: {}, params_count: {}, params: {:?}", 
                        matched_route.route, 
                        matched_route.matcher_type, 
                        matched_route.active_strategy.name,
                        matched_route.active_strategy.middleware.len(),
                        matched_route.params.len(),
                        matched_route.params
                    );
                    
                    // Log detailed middleware information
                    for (i, middleware) in matched_route.active_strategy.middleware.iter().enumerate() {
                        module_log_debug!("middleware detail - middleware_index: {}, middleware_config: {:?}", i, middleware);
                    }
                }
                Ok(None) => {
                    module_log_info!("no route matched - path: {}", path);
                }
                Err(e) => {
                    module_log_error!("error getting route - path: {}, error: {}", path, e);
                }
            }
        } else {
            module_log_warn!("HttpWardContext not found in request context");
        }

        // Call next middleware / inner service
        let res = next.run(ctx, req).await?;

        // Log response status
        module_log_info!("response produced - status: {}", res.status());

        Ok(res)
    }

    fn name(&self) -> Option<&'static str> {
        Some("HttpWardLogLayer")
    }
}
