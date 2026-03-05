use rama::{
    http::{Request as RamaRequest, Response as RamaResponse, Body as RamaBody, header, HeaderValue, Version},
    http::client::EasyHttpWebClient,
    http::service::client::HttpClientExt,
};
use http::Uri;
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("http client error: {0}")]
    Http(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("upstream error: {0}")]
    Upstream(String),
}

/// HTTP/HTTPS proxy handler
#[derive(Clone, Debug)]
pub struct ProxyHandler {
    client: EasyHttpWebClient,
}

impl ProxyHandler {
    pub fn new() -> Self {
        let client = EasyHttpWebClient::new();
        Self { client }
    }
    
    /// Proxy HTTP request to upstream
    pub async fn proxy_request(
        &self,
        mut req: RamaRequest<RamaBody>,
        upstream_base: &str,
        matched_path_prefix: &str,
    ) -> Result<RamaResponse<RamaBody>, ProxyError> {
        // Build upstream URL
        let new_uri = Self::build_upstream_url(upstream_base, req.uri(), matched_path_prefix)?;
        *req.uri_mut() = new_uri;

        // Ensure Host header for upstream
        let authority = req.uri().authority().map(|a| a.to_string());
        if !req.headers().contains_key(header::HOST) {
            if let Some(authority_str) = authority {
                req.headers_mut().insert(
                    header::HOST,
                    HeaderValue::from_str(&authority_str)
                        .map_err(|e| ProxyError::Upstream(format!("Invalid host header: {}", e)))?,
                );
            }
        }

        // Send request using Rama HTTP client
        let resp = self.client
            .request(req.method().clone(), req.uri().clone())
            .headers(req.headers().clone())
            .send(rama::Context::default())
            .await
            .map_err(|e| ProxyError::Http(Box::new(e)))?;
        
        Ok(resp)
    }
    
    /// Build upstream URI by combining base and original request path+query
    fn build_upstream_url(base: &str, orig: &Uri, _matched_path_prefix: &str) -> Result<Uri, ProxyError> {
        let base_url = Url::parse(base)
            .map_err(|e| ProxyError::InvalidUrl(format!("bad base url: {}", e)))?;
            
        // Use the full original path and query
        let path_and_query = if let Some(q) = orig.query() {
            format!("{}?{}", orig.path(), q)
        } else {
            orig.path().to_string()
        };
        
        let joined = base_url.join(&path_and_query)
            .map_err(|e| ProxyError::InvalidUrl(format!("join error: {}", e)))?;
            
        Ok(joined.as_str().parse()
            .map_err(|e| ProxyError::InvalidUrl(format!("uri parse: {}", e)))?)
    }
    
    /// Check if request is for WebSocket upgrade
    pub fn is_websocket_upgrade(req: &RamaRequest<RamaBody>) -> bool {
        // Check Upgrade header
        if let Some(upgrade) = req.headers().get(header::UPGRADE) {
            if let Ok(upgrade_str) = upgrade.to_str() {
                if upgrade_str.to_ascii_lowercase().contains("websocket") {
                    // Check Connection header
                    if let Some(connection) = req.headers().get(header::CONNECTION) {
                        if let Ok(conn_str) = connection.to_str() {
                            return conn_str.to_ascii_lowercase().contains("upgrade");
                        }
                    }
                }
            }
        }
        false
    }
    
    /// Check if request is gRPC
    pub fn is_grpc(req: &RamaRequest<RamaBody>) -> bool {
        // Check content-type for gRPC
        if let Some(content_type) = req.headers().get(header::CONTENT_TYPE) {
            if let Ok(ct_str) = content_type.to_str() {
                if ct_str.starts_with("application/grpc") {
                    return true;
                }
            }
        }
        // HTTP/2 requests are candidates for gRPC
        req.version() == Version::HTTP_2
    }
}

impl Default for ProxyHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rama::http::{Method, Uri};
    
    #[test]
    fn test_build_upstream_url() {
        let base = "http://backend:8080/api";
        let orig_uri = "/api/users/123?active=true".parse::<Uri>().unwrap();
        
        let result = ProxyHandler::build_upstream_url(base, &orig_uri, "/api").unwrap();
        assert_eq!(result.to_string(), "http://backend:8080/api/users/123?active=true");
    }
    
    #[test]
    fn test_build_upstream_url_user_case() {
        // Test case from user: path "/ip" should proxy to "http://zerex222.ru:8080/ip"
        let base = "http://zerex222.ru:8080/ip";
        let orig_uri = "/ip".parse::<Uri>().unwrap();
        
        let result = ProxyHandler::build_upstream_url(base, &orig_uri, "/ip").unwrap();
        assert_eq!(result.to_string(), "http://zerex222.ru:8080/ip");
        
        // Test case: path "/ip/ololo" should proxy to "http://zerex222.ru:8080/ip/ololo"
        let orig_uri2 = "/ip/ololo".parse::<Uri>().unwrap();
        let result2 = ProxyHandler::build_upstream_url(base, &orig_uri2, "/ip").unwrap();
        assert_eq!(result2.to_string(), "http://zerex222.ru:8080/ip/ololo");
    }
    
    #[test]
    fn test_websocket_detection() {
        let mut req = RamaRequest::builder()
            .method(Method::GET)
            .uri("/ws")
            .header("Upgrade", "websocket")
            .header("Connection", "upgrade")
            .body(RamaBody::empty())
            .unwrap();
            
        assert!(ProxyHandler::is_websocket_upgrade(&req));
        
        // Test case-insensitive
        let mut req2 = RamaRequest::builder()
            .method(Method::GET)
            .uri("/ws")
            .header("Upgrade", "WebSocket")
            .header("Connection", "Upgrade")
            .body(RamaBody::empty())
            .unwrap();
            
        assert!(ProxyHandler::is_websocket_upgrade(&req2));
    }
    
    #[test]
    fn test_grpc_detection() {
        let req = RamaRequest::builder()
            .method(Method::POST)
            .uri("/grpc.service")
            .header("Content-Type", "application/grpc")
            .body(RamaBody::empty())
            .unwrap();
            
        assert!(ProxyHandler::is_grpc(&req));
    }
}
