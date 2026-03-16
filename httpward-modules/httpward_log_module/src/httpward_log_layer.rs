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
use httpward_core::{get_module_config_from_current_crate, module_log_error, module_log_warn, module_log_info, module_log_debug};

// Import for configuration
use serde::Deserialize;

/// Configuration for HttpWardLogLayer
#[derive(Debug, Clone, Deserialize)]
pub struct HttpWardLogConfig {
    pub level: String,
    pub tag: Option<String>,
    pub format: Option<String>,
    pub include_request_body: Option<bool>,
    pub include_response_body: Option<bool>,
}

impl Default for HttpWardLogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            tag: None,
            format: Some("text".to_string()),
            include_request_body: Some(false),
            include_response_body: Some(false),
        }
    }
}

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
        
        // Get configuration from context using universal function (1 line!)
        // Automatically uses current crate name = "httpward_log_module"
        let config = get_module_config_from_current_crate!(HttpWardLogConfig, &ctx, &req)
            .unwrap_or_default();
        
        // Log the configuration for debugging
        module_log_debug!("HttpWardLogLayer config loaded: {:?}", config);
        
        // Use configuration from middleware or fallback to tag from layer
        let effective_tag = config.tag.as_ref().or(self.tag.as_ref());
        let log_level = &config.level;
        
        // Log incoming request line
        let uri = req.uri().to_string();
        if let Some(tag) = effective_tag {
            module_log_info!("[{}] incoming request - URI: {}, tag: {}", log_level, uri, tag);
        } else {
            module_log_info!("[{}] incoming request - URI: {}", log_level, uri);
        }

        // Log HttpWardContext information
        if let Some(httpward_ctx) = ctx.get::<HttpWardContext>() {
            // Log current site information
            if let Some(current_site) = &httpward_ctx.current_site {
                module_log_info!("[{}] current site detected - site_name: {}", log_level, current_site.site_name());
            } else {
                module_log_info!("[{}] no current site set", log_level);
            }

            // Log route information
            let path = req.uri().path();
            match httpward_ctx.get_route(path) {
                Ok(Some(matched_route)) => {
                    module_log_info!("[{}] route matched with strategy - route_type: {:?}, matcher_type: {:?}, strategy_name: {}, middleware_count: {}, params_count: {}, params: {:?}", 
                        log_level,
                        matched_route.route, 
                        matched_route.matcher_type, 
                        matched_route.active_strategy.name,
                        matched_route.active_strategy.middleware.len(),
                        matched_route.params.len(),
                        matched_route.params
                    );
                    
                    // Log detailed middleware information
                    for (i, middleware) in matched_route.active_strategy.middleware.iter().enumerate() {
                        module_log_debug!("[{}] middleware detail - middleware_index: {}, middleware_config: {:?}", log_level, i, middleware);
                    }
                }
                Ok(None) => {
                    module_log_info!("[{}] no route matched - path: {}", log_level, path);
                }
                Err(e) => {
                    module_log_error!("[{}] error getting route - path: {}, error: {}", log_level, path, e);
                }
            }
        } else {
            module_log_warn!("[{}] HttpWardContext not found in request context", log_level);
        }

        // Call next middleware / inner service
        let res = next.run(ctx, req).await?;

        // Log response status
        module_log_info!("[{}] response produced - status: {}", log_level, res.status());

        Ok(res)
    }

    fn name(&self) -> Option<&'static str> {
        Some("HttpWardLogLayer")
    }
}
