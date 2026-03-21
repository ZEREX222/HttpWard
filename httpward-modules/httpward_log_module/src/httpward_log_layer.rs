// File: httpward-modules/httpward_log_module/src/httpward_log_layer

use async_trait::async_trait;
use httpward_core::core::HttpWardContext;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::next::Next;
use httpward_core::httpward_middleware::types::BoxError;
use httpward_core::module_logging::ModuleLogger;
use httpward_core::{module_log_debug, module_log_error, module_log_info, module_log_warn};
use rama::Context;
use rama::http::{Body, Request, Response};
use std::fmt::Debug;

// Import for configuration
use serde::Deserialize;

/// Configuration for HttpWardLogLayer
#[derive(Debug, Clone, Deserialize)]
pub struct HttpWardLogConfig {
    /// Show basic request information (URI, method, etc.)
    #[serde(default)]
    pub show_request: bool,

    /// Log client IP address from HttpWardContext
    #[serde(default)]
    pub log_client_ip: bool,

    /// Log current site information from HttpWardContext
    #[serde(default)]
    pub log_current_site: bool,

    /// Log route matching details from HttpWardContext
    #[serde(default)]
    pub log_route_info: bool,

    /// Log strategy information for matched routes
    #[serde(default)]
    pub log_strategy_info: bool,

    /// Log middleware details for active strategy
    #[serde(default)]
    pub log_middleware_details: bool,

    /// Log URL parameters extracted from route matching
    #[serde(default)]
    pub log_url_params: bool,

    /// Log request headers from HttpWardContext
    #[serde(default)]
    pub log_request_headers: bool,

    /// Log content type information
    #[serde(default)]
    pub log_content_type: bool,

    /// Log response status code
    #[serde(default)]
    pub log_response_status: bool,

    /// Log server instance information
    #[serde(default)]
    pub log_server_info: bool,
}

impl Default for HttpWardLogConfig {
    fn default() -> Self {
        Self {
            show_request: true,
            log_client_ip: false,
            log_current_site: false,
            log_route_info: false,
            log_strategy_info: false,
            log_middleware_details: false,
            log_url_params: false,
            log_request_headers: false,
            log_content_type: false,
            log_response_status: false,
            log_server_info: false,
        }
    }
}

/// Simple logging middleware for HttpWard with custom module logging.
#[derive(Clone, Debug, Default)]
pub struct HttpWardLogLayer;

impl HttpWardLogLayer {
    pub fn new() -> Self {
        Self
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

        let httpward_ctx = ctx.get::<HttpWardContext>();
        let matched_for_logging = httpward_ctx.and_then(|c| c.matched_route.clone());
        let request_path = req.uri().path();

        let config = if let Some(httpward_ctx) = httpward_ctx {
            match httpward_ctx.middleware_config_typed_from_matched_route::<HttpWardLogConfig>(
                env!("CARGO_PKG_NAME"),
            ) {
                Ok(Some(config)) => {
                    module_log_debug!(
                        "HttpWardLogLayer config loaded from HttpWardContext.matched_route: {:?}",
                        config
                    );
                    config
                }
                Ok(None) => {
                    module_log_debug!(
                        "HttpWardLogLayer config not found in HttpWardContext.matched_route, using defaults"
                    );
                    std::sync::Arc::new(HttpWardLogConfig::default())
                }
                Err(e) => {
                    module_log_error!(
                        "Failed to parse HttpWardLogLayer configuration from HttpWardContext.matched_route: {}, using defaults",
                        e
                    );
                    std::sync::Arc::new(HttpWardLogConfig::default())
                }
            }
        } else {
            module_log_warn!("HttpWardContext not found in request context");
            std::sync::Arc::new(HttpWardLogConfig::default())
        };

        // Log basic request information if enabled
        if config.show_request {
            let method = req.method();
            let uri = req.uri();
            module_log_info!(
                "Incoming request - Method: {}, URI: {}, Path: {}",
                method,
                uri,
                uri.path()
            );
        }

        // Get HttpWardContext for detailed logging
        if let Some(httpward_ctx) = httpward_ctx {
            // Log client IP address if enabled
            if config.log_client_ip {
                module_log_info!("Client IP: {}", httpward_ctx.client_ip);
            }

            // Log server instance information if enabled
            if config.log_server_info {
                let site_count = httpward_ctx.server_instance.site_managers.len();
                module_log_info!(
                    "Server instance - Total sites: {}, Server available: true",
                    site_count
                );
            }

            // Log current site information if enabled
            if config.log_current_site {
                if let Some(current_site) = &httpward_ctx.current_site {
                    module_log_info!(
                        "Current site - Name: {}, Has domains: {}",
                        current_site.site_domains(),
                        current_site.site_config().has_domains()
                    );
                } else {
                    module_log_info!("No current site set");
                }
            }

            // Log content type information if enabled
            if config.log_content_type {
                module_log_info!(
                    "Content types - Request: {}, Response: {}",
                    httpward_ctx.request_content_type,
                    httpward_ctx.response_content_type
                );
            }

            // Log request headers if enabled
            if config.log_request_headers {
                let header_count = httpward_ctx.request_headers.len();
                module_log_info!("Request headers count: {}", header_count);

                if let Some(user_agent) = httpward_ctx.request_headers.get("user-agent")
                    && let Ok(ua) = user_agent.to_str()
                {
                    module_log_info!("User-Agent: {}", ua);
                }
                if let Some(host) = httpward_ctx.request_headers.get("host")
                    && let Ok(h) = host.to_str()
                {
                    module_log_info!("Host: {}", h);
                }
            }
        }

        // Log route / strategy info from cached matched route in HttpWardContext.
        if config.log_route_info {
            if let Some(matched_route) = &matched_for_logging {
                module_log_info!(
                    "Route matched - Path: {}, Route: {:?}",
                    request_path,
                    matched_route.route
                );

                if config.log_url_params && !matched_route.params.is_empty() {
                    module_log_info!(
                        "URL parameters - Count: {}, Parameters: {:?}, Matcher type: {:?}",
                        matched_route.params.len(),
                        matched_route.params,
                        matched_route.matcher_type
                    );
                }

                if config.log_strategy_info {
                    module_log_info!(
                        "Active strategy - Name: {}, Middleware count: {}",
                        matched_route.active_strategy.name,
                        matched_route.active_strategy.middleware.len()
                    );
                }

                if config.log_middleware_details {
                    for (i, mw) in matched_route.active_strategy.middleware.iter().enumerate() {
                        module_log_info!("Middleware[{}] - Type: {:?}", i, mw);
                    }
                }
            } else {
                module_log_debug!(
                    "Route info requested but HttpWardContext.matched_route is not set"
                );
            }
        }

        // Call next middleware / inner service
        let res = next.run(ctx, req).await?;

        // Log response status if enabled
        if config.log_response_status {
            module_log_info!("Response status: {}", res.status());
        }

        Ok(res)
    }

    fn name(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_NAME"))
    }
}
