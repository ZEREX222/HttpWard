use rama::{
    layer::Layer,
    service::Service,
    Context,
    http::{Request as RamaRequest, Response as RamaResponse, Body as RamaBody, StatusCode},
};
use std::fmt::Debug;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;
use crate::middleware::core::HttpWardContext;
use crate::config::Route;
use super::{
    matcher::{RouteMatcher, MatcherError},
    proxy::{ProxyHandler, ProxyError},
    websocket::{WebSocketHandler, WebSocketError},
};

#[derive(Error, Debug)]
pub enum RouteError {
    #[error("matcher error: {0}")]
    Matcher(#[from] MatcherError),
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
}

impl<S> RouteService<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            proxy_handler: ProxyHandler::new(),
            websocket_handler: WebSocketHandler::new(),
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
                HttpWardContext::new(
                    std::net::SocketAddr::from(([127, 0, 0, 1], 8080)),
                    Arc::new(crate::config::GlobalConfig::default()),
                )
            });

        debug!(?httpward_ctx, "HttpWard context retrieved");
        let routes = if let Some(site) = &httpward_ctx.site {
            site.routes.clone()
        } else {
            // No site config, pass to inner service
            return self.inner.serve(ctx, request).await;
        };

        if routes.is_empty() {
            return self.inner.serve(ctx, request).await;
        }

        // Create matcher for current routes
        let matcher = match RouteMatcher::new(routes) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("Failed to create route matcher: {}", e);
                return Ok(RamaResponse::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(RamaBody::from("Internal routing error"))
                    .unwrap());
            }
        };

        let path = request.uri().path();
        
        // Try to match route
        match matcher.match_route(path) {
            Ok(matched_route) => {
                // Handle different route types
                match matched_route.route {
                    Route::Proxy { ref backend, .. } => {
                        // Check if WebSocket upgrade
                        if ProxyHandler::is_websocket_upgrade(&request) {
                            match WebSocketHandler::http_to_ws_url(&backend) {
                                Ok(ws_url) => {
                                    match self.websocket_handler.proxy_websocket(request, &ws_url).await {
                                        Ok(response) => return Ok(response),
                                        Err(e) => {
                                            tracing::error!("WebSocket proxy error: {}", e);
                                            return Ok(RamaResponse::builder()
                                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                                .body(RamaBody::from("WebSocket proxy error"))
                                                .unwrap());
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Failed to convert HTTP URL to WebSocket URL: {}", e);
                                    return Ok(RamaResponse::builder()
                                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                                        .body(RamaBody::from("Invalid WebSocket URL"))
                                        .unwrap());
                                }
                            }
                        }
                        
                        // Regular HTTP proxy
                        let matched_path_prefix = match &matched_route.route {
                            Route::Proxy { r#match, .. } => r#match.path.as_deref().unwrap_or(""),
                            _ => "",
                        };
                        match self.proxy_handler.proxy_request(request, &backend, matched_path_prefix).await {
                            Ok(response) => return Ok(response),
                            Err(e) => {
                                tracing::error!("Proxy error: {}", e);
                                return Ok(RamaResponse::builder()
                                    .status(StatusCode::BAD_GATEWAY)
                                    .body(RamaBody::from("Proxy error"))
                                    .unwrap());
                            }
                        }
                    }
                    Route::Static { static_dir, .. } => {
                        match self.handle_static(request, &static_dir).await {
                            Ok(response) => return Ok(response),
                            Err(e) => {
                                tracing::error!("Static file error: {}", e);
                                return Ok(RamaResponse::builder()
                                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                                    .body(RamaBody::from("Static file error"))
                                    .unwrap());
                            }
                        }
                    }
                    Route::Redirect { redirect, .. } => {
                        match self.handle_redirect(request, &redirect).await {
                            Ok(response) => return Ok(response),
                            Err(e) => {
                                tracing::error!("Redirect error: {}", e);
                                return Ok(RamaResponse::builder()
                                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                                    .body(RamaBody::from("Redirect error"))
                                    .unwrap());
                            }
                        }
                    }
                }
            }
            Err(MatcherError::NoMatch) => {
                // No route matched, pass to inner service
                return self.inner.serve(ctx, request).await;
            }
            Err(e) => {
                tracing::error!("Route matching error: {}", e);
                return Ok(RamaResponse::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(RamaBody::from("Routing error"))
                    .unwrap());
            }
        }
    }
}

impl<S> RouteService<S> {
    /// Handle static file serving
    async fn handle_static(
        &self,
        request: RamaRequest<RamaBody>,
        static_dir: &std::path::PathBuf,
    ) -> Result<RamaResponse<RamaBody>, RouteError> {
        use tokio::fs;
        
        let path = request.uri().path();
        let path = path.trim_start_matches('/');
        
        // Prevent directory traversal
        if path.contains("..") {
            return Ok(RamaResponse::builder()
                .status(StatusCode::FORBIDDEN)
                .body(RamaBody::from("Forbidden"))
                .unwrap());
        }
        
        let file_path = static_dir.join(path);
        
        // Check if file exists and is within static_dir
        match fs::metadata(&file_path).await {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Ok(RamaResponse::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(RamaBody::from("Not Found"))
                        .unwrap());
                }
                
                // Try to determine content type
                let content_type = self.guess_content_type(&file_path);
                
                match fs::read(&file_path).await {
                    Ok(contents) => {
                        let mut response = RamaResponse::builder()
                            .status(StatusCode::OK);
                            
                        if let Some(ct) = content_type {
                            response = response.header("Content-Type", ct);
                        }
                        
                        Ok(response
                            .body(RamaBody::from(contents))
                            .unwrap())
                    }
                    Err(e) => {
                        tracing::error!("Failed to read static file: {}", e);
                        Ok(RamaResponse::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(RamaBody::from("Internal Server Error"))
                            .unwrap())
                    }
                }
            }
            Err(_) => {
                Ok(RamaResponse::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(RamaBody::from("Not Found"))
                    .unwrap())
            }
        }
    }
    
    /// Handle redirects
    async fn handle_redirect(
        &self,
        _request: RamaRequest<RamaBody>,
        redirect: &crate::config::Redirect,
    ) -> Result<RamaResponse<RamaBody>, RouteError> {
        let location = redirect.to.clone();
        
        Ok(RamaResponse::builder()
            .status(StatusCode::from_u16(redirect.code).unwrap_or(StatusCode::FOUND))
            .header("Location", location)
            .body(RamaBody::empty())
            .unwrap())
    }
    
    /// Guess content type based on file extension
    fn guess_content_type(&self, path: &std::path::Path) -> Option<&'static str> {
        let extension = path.extension()?.to_str()?;
        
        match extension.to_lowercase().as_str() {
            "html" => Some("text/html"),
            "css" => Some("text/css"),
            "js" => Some("application/javascript"),
            "json" => Some("application/json"),
            "xml" => Some("application/xml"),
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "svg" => Some("image/svg+xml"),
            "pdf" => Some("application/pdf"),
            "txt" => Some("text/plain"),
            _ => None,
        }
    }
}
