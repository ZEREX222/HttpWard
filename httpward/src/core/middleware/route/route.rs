use rama::{
    layer::Layer,
    service::Service,
    Context,
    http::{Request as RamaRequest, Response as RamaResponse, Body as RamaBody, StatusCode},
};
use std::fmt::Debug;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error};
use httpward_core::config::{GlobalConfig, Redirect, Route};
use httpward_core::core::HttpWardContext;
use httpward_core::core::server_models::server_instance::ServerInstance;
use httpward_core::core::server_models::listener::ListenerKey;
use crate::core::error::ErrorHandler;
use super::{
    proxy::{ProxyHandler, ProxyError},
    websocket::{WebSocketHandler, WebSocketError},
    static_files,
};
use httpward_core::core::server_models::{SiteManager, SiteManagerError};

#[derive(Error, Debug)]
pub enum RouteError {
    #[error("site manager error: {0}")]
    SiteManager(#[from] SiteManagerError),
    #[error("proxy error: {0}")]
    Proxy(#[from] ProxyError),
    #[error("websocket error: {0}")]
    WebSocket(#[from] WebSocketError),
    #[error("static file error: {0}")]
    Static(String),
    #[error("redirect error: {0}")]
    Redirect(String),
    #[error("other: {0}")]
    Other(String),
}

/// Layer for routing
#[derive(Clone, Debug)]
pub struct RouteLayer;

impl RouteLayer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RouteLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for RouteLayer {
    type Service = RouteService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RouteService::new(inner)
    }
}

/// Service for routing
#[derive(Clone, Debug)]
pub struct RouteService<S> {
    inner: S,
    proxy_handler: ProxyHandler,
    websocket_handler: WebSocketHandler,
    error_handler: ErrorHandler,
}

impl<S> RouteService<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            proxy_handler: ProxyHandler::new(),
            websocket_handler: WebSocketHandler::new(),
            error_handler: ErrorHandler::default(),
        }
    }
}

impl<S, State> Service<State, RamaRequest<RamaBody>> for RouteService<S>
where
    S: Service<State, RamaRequest<RamaBody>, Response = RamaResponse<RamaBody>> + Send + Sync + 'static,
    S::Error: Debug + Send + 'static,
    State: Clone + Send + Sync + 'static,
{
    type Response = RamaResponse<RamaBody>;
    type Error = S::Error;

    async fn serve(
        &self,
        ctx: Context<State>,
        request: RamaRequest<RamaBody>,
    ) -> Result<Self::Response, Self::Error> {
        // Extract HttpWardContext from the main context
        let httpward_ctx = ctx.get::<HttpWardContext>()
            .cloned()
            .unwrap_or_else(|| {
                // Fallback context if not present
                let server_instance = ServerInstance {
                    bind: ListenerKey {
                        host: "127.0.0.1".to_string(),
                        port: 8080,
                    },
                    site_managers: vec![],
                    global: GlobalConfig::default(),
                };
                HttpWardContext::new(
                    std::net::SocketAddr::from(([127, 0, 0, 1], 8080)),
                    Arc::new(server_instance),
                )
            });

        debug!(?httpward_ctx, "HttpWard context retrieved");
        let current_site = if let Some(site_manager) = &httpward_ctx.current_site {
            site_manager.clone()
        } else {
            // No site config, pass to inner service
            return self.inner.serve(ctx, request).await;
        };

        let path = request.uri().path().to_string();
        tracing::debug!("Trying to match route for path: {}", path);
        tracing::debug!("Available routes: {:?}", current_site.routes());
        
        // Try to match route using SiteManager
        match current_site.get_route(&path) {
            Ok(matched_route) => {
                tracing::debug!("Route matched: {:?}", matched_route.route);
                // Handle different route types
                match &*matched_route.route {
                    Route::Proxy { backend, .. } => {
                        // Check if WebSocket upgrade
                        let is_websocket = ProxyHandler::is_websocket_upgrade(&request);
                        
                        if is_websocket {
                            match WebSocketHandler::http_to_ws_url(&backend) {
                                Ok(ws_url) => {
                                    match self.websocket_handler.proxy_websocket(request, &ws_url).await {
                                        Ok(response) => return Ok(response),
                                        Err(e) => {
                                            tracing::error!("WebSocket proxy error: {}", e);
                                            return Ok(self.error_handler.create_error_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
                                                .unwrap_or_else(|_| RamaResponse::builder()
                                                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                                                    .body(RamaBody::from("WebSocket proxy error"))
                                                    .unwrap()));
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Failed to convert HTTP URL to WebSocket URL: {}", e);
                                    return Ok(self.error_handler.create_error_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
                                        .unwrap_or_else(|_| RamaResponse::builder()
                                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                                            .body(RamaBody::from("Invalid WebSocket URL"))
                                            .unwrap()));
                                }
                            }
                        } else {
                            match self.proxy_handler.proxy_request(request, &backend, &matched_route.params).await {
                                Ok(response) => return Ok(response),
                                Err(e) => {
                                    tracing::error!("Proxy error: {}", e);
                                    return Ok(self.error_handler.create_error_response_with_code(StatusCode::BAD_GATEWAY)
                                        .unwrap_or_else(|_| RamaResponse::builder()
                                            .status(StatusCode::BAD_GATEWAY)
                                            .body(RamaBody::from("Proxy error"))
                                            .unwrap()));
                                }
                            }
                        }
                    }
                    Route::Static { static_dir, .. } => {
                        match static_files::handle_static(&request, static_dir, &matched_route).await {
                            Ok(response) => return Ok(response),
                            Err(e) => {
                                error!("Static file error: {}", e);
                                return Ok(self.error_handler.create_error_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
                                    .unwrap_or_else(|_| RamaResponse::builder()
                                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                                        .body(RamaBody::from("Static file error"))
                                        .unwrap()));
                            }
                        }
                    }
                    Route::Redirect { redirect, .. } => {
                        match self.handle_redirect(request, &redirect).await {
                            Ok(response) => return Ok(response),
                            Err(e) => {
                                error!("Redirect error: {}", e);
                                return Ok(self.error_handler.create_error_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
                                    .unwrap_or_else(|_| RamaResponse::builder()
                                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                                        .body(RamaBody::from("Redirect error"))
                                        .unwrap()));
                            }
                        }
                    }
                }
            }
            Err(SiteManagerError::NoMatch) => {
                debug!("No route matched for path: {}", path);
                // No route matched, pass to inner service
                return self.inner.serve(ctx, request).await;
            }
            Err(e) => {
                error!("Route matching error: {}", e);
                return Ok(self.error_handler.create_error_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
                    .unwrap_or_else(|_| RamaResponse::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(RamaBody::from("Routing error"))
                        .unwrap()));
            }
        }
    }
}

impl<S> RouteService<S> {
    /// Handle redirects
    async fn handle_redirect(
        &self,
        _request: RamaRequest<RamaBody>,
        redirect: &Redirect,
    ) -> Result<RamaResponse<RamaBody>, RouteError> {
        let location = redirect.to.clone();
        
        Ok(RamaResponse::builder()
            .status(StatusCode::from_u16(redirect.code).unwrap_or(StatusCode::FOUND))
            .header("Location", location)
            .body(RamaBody::empty())
            .unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use httpward_core::config::{Match, SiteConfig};

    #[tokio::test]
    async fn test_static_route_matching() {
        // Create site config with static routes
        let site_config = SiteConfig {
            domain: "test-site".to_string(),
            domains: vec![],
            listeners: vec![],
            routes: vec![
                Route::Static {
                    r#match: Match {
                        path: Some("/site".to_string()),
                        path_regex: None,
                    },
                    static_dir: PathBuf::from("C:/test/html"),
                    strategy: None,
                    strategies: None,
                },
                // For subpaths, we need wildcard route
                Route::Static {
                    r#match: Match {
                        path: Some("/site/{*path}".to_string()),
                        path_regex: None,
                    },
                    static_dir: PathBuf::from("C:/test/html"),
                    strategy: None,
                    strategies: None,
                },
            ],
            strategy: None,
            strategies: std::collections::HashMap::new(),
        };
        
        // Create SiteManager
        let site_manager = SiteManager::new(std::sync::Arc::new(site_config)).unwrap();
        
        // Test matching exact route
        let result = site_manager.get_route("/site");
        assert!(result.is_ok(), "Failed to match /site route");
        
        let matched_route = result.unwrap();
        assert!(matches!(&*matched_route.route, &Route::Static { .. }));
        
        // Test with subpath (should match wildcard route)
        let result2 = site_manager.get_route("/site/style.css");
        assert!(result2.is_ok(), "Failed to match /site/style.css route");
        
        let matched_route2 = result2.unwrap();
        assert!(matches!(&*matched_route2.route, &Route::Static { .. }));
        assert_eq!(matched_route2.params.get("path"), Some(&"style.css".to_string()));
        
        // Test non-matching path
        let result3 = site_manager.get_route("/other");
        assert!(result3.is_err(), "Should not match /other path");
    }
}
