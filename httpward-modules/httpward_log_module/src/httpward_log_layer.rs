// File: httpward-modules/httpward_log_module/src/httpward_log_layer

use async_trait::async_trait;
use httpward_core::core::HttpWardContext;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::next::Next;
use httpward_core::httpward_middleware::types::BoxError;
use httpward_core::module_logging::ModuleLogger;
use httpward_core::{
    get_module_config_from_current_crate, module_log_debug, module_log_error, module_log_info,
    module_log_warn,
};
use rama::Context;
use rama::http::{Body, Request, Response};
use std::fmt::Debug;

// Import for configuration
use serde::Deserialize;

/// Configuration for HttpWardLogLayer
#[derive(Debug, Clone, Deserialize)]
pub struct HttpWardLogConfig {
    /// Show basic request information (URI, method, etc.)
    pub show_request: bool,
    
    /// Log client IP address from HttpWardContext
    pub log_client_ip: bool,
    
    /// Log current site information from HttpWardContext
    pub log_current_site: bool,
    
    /// Log route matching details from HttpWardContext
    pub log_route_info: bool,
    
    /// Log strategy information for matched routes
    pub log_strategy_info: bool,
    
    /// Log middleware details for active strategy
    pub log_middleware_details: bool,
    
    /// Log URL parameters extracted from route matching
    pub log_url_params: bool,
    
    /// Log request headers from HttpWardContext
    pub log_request_headers: bool,
    
    /// Log content type information
    pub log_content_type: bool,
    
    /// Log fingerprint information (header_fp, ja4_fp)
    pub log_fingerprints: bool,
    
    /// Log response status code
    pub log_response_status: bool,
    
    /// Log server instance information
    pub log_server_info: bool,
}

impl Default for HttpWardLogConfig {
    fn default() -> Self {
        Self {
            show_request: true,
            log_client_ip: true,
            log_current_site: true,
            log_route_info: true,
            log_strategy_info: false,
            log_middleware_details: false,
            log_url_params: false,
            log_request_headers: false,
            log_content_type: false,
            log_fingerprints: false,
            log_response_status: true,
            log_server_info: false,
        }
    }
}

/// Simple logging middleware for HttpWard with custom module logging.
#[derive(Clone, Debug, Default)]
pub struct HttpWardLogLayer {}

impl HttpWardLogLayer {
    pub fn new() -> Self {
        Self {}
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

        // Get configuration from context using universal function
        let config = get_module_config_from_current_crate!(HttpWardLogConfig, &ctx, &req)
            .unwrap_or_default();

        module_log_debug!("HttpWardLogLayer config loaded: {:?}", config);

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
        if let Some(httpward_ctx) = ctx.get::<HttpWardContext>() {
            
            // Log client IP address if enabled
            if config.log_client_ip {
                module_log_info!(
                    "Client IP: {}",
                    httpward_ctx.client_ip
                );
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
                        current_site.site_name(),
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

            // Log fingerprint information if enabled
            if config.log_fingerprints {
                module_log_info!(
                    "Fingerprints - Header FP: {:?}, JA4 FP: {:?}",
                    httpward_ctx.header_fp,
                    httpward_ctx.ja4_fp
                );
            }

            // Log request headers if enabled
            if config.log_request_headers {
                let header_count = httpward_ctx.request_headers.len();
                module_log_info!(
                    "Request headers count: {}",
                    header_count
                );
                
                // Log some key headers
                if let Some(user_agent) = httpward_ctx.request_headers.get("user-agent") {
                    if let Ok(user_agent_str) = user_agent.to_str() {
                        module_log_info!(
                            "User-Agent: {}",
                            user_agent_str
                        );
                    }
                }
                
                if let Some(host) = httpward_ctx.request_headers.get("host") {
                    if let Ok(host_str) = host.to_str() {
                        module_log_info!(
                            "Host: {}",
                            host_str
                        );
                    }
                }
            }

            // Log route matching information if enabled
            if config.log_route_info {
                let path = req.uri().path();
                match httpward_ctx.get_route(path) {
                    Ok(Some(matched_route)) => {
                        module_log_info!(
                            "Route matched - Path: {}, Route type: {:?}, Matcher type: {:?}",
                            path,
                            matched_route.route,
                            matched_route.matcher_type
                        );

                        // Log URL parameters if enabled
                        if config.log_url_params && !matched_route.params.is_empty() {
                            module_log_info!(
                                "URL parameters - Count: {}, Parameters: {:?}",
                                matched_route.params.len(),
                                matched_route.params
                            );
                        }

                        // Log strategy information if enabled
                        if config.log_strategy_info {
                            module_log_info!(
                                "Active strategy - Name: {}, Middleware count: {}",
                                matched_route.active_strategy.name,
                                matched_route.active_strategy.middleware.len()
                            );
                        }

                        // Log middleware details if enabled
                        if config.log_middleware_details {
                            for (i, middleware) in matched_route.active_strategy.middleware.iter().enumerate() {
                                module_log_info!(
                                    "Middleware[{}] - Type: {:?}",
                                    i,
                                    middleware
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        module_log_info!(
                            "No route matched for path: {}",
                            path
                        );
                    }
                    Err(e) => {
                        module_log_error!(
                            "Error getting route for path: {}, Error: {}",
                            path,
                            e
                        );
                    }
                }
            }
        } else {
            module_log_warn!("HttpWardContext not found in request context");
        }

        // Call next middleware / inner service
        let res = next.run(ctx, req).await?;

        // Log response status if enabled
        if config.log_response_status {
            module_log_info!(
                "Response status: {}",
                res.status()
            );
        }

        Ok(res)
    }

    fn name(&self) -> Option<&'static str> {
        Some("HttpWardLogLayer")
    }
}
