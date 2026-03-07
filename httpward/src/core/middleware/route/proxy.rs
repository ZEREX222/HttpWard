use rama::{
    http::{Request as RamaRequest, Response as RamaResponse, Body as RamaBody, header, HeaderValue, Version},
    http::client::EasyHttpWebClient,
    http::service::client::HttpClientExt,
};
use http::Uri;
use thiserror::Error;
use url::Url;
use std::collections::HashMap;

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

    /// Process backend URL with matcher parameters
    /// Replaces placeholders like {param}, {*any}, and {1}, {2} (regex groups) with actual values from params
    fn process_backend_url_with_params(
        backend: &str,
        params: &HashMap<String, String>,
    ) -> Result<String, ProxyError> {
        let mut result = backend.to_string();
        
        // Replace named parameters like {param} and {*any}
        for (key, value) in params {
            let placeholder = format!("{{{}}}", key);
            result = result.replace(&placeholder, value);
            
            // Also handle wildcard parameters {*any}
            let wildcard_placeholder = format!("{{*{}}}", key);
            result = result.replace(&wildcard_placeholder, value);
        }
        
        Ok(result)
    }
    
    /// Proxy HTTP request to upstream
    pub async fn proxy_request(
        &self,
        mut req: RamaRequest<RamaBody>,
        backend: &str,
        params: &HashMap<String, String>,
    ) -> Result<RamaResponse<RamaBody>, ProxyError> {
        // Process backend URL with matcher parameters
        let processed_backend = Self::process_backend_url_with_params(backend, params)?;
        
        // Build upstream URL
        let new_uri = Self::build_upstream_url(&processed_backend, req.uri())?;
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
    
    /// Build upstream URI by combining processed backend URL with original request
    fn build_upstream_url(
        backend: &str,
        orig: &Uri,
    ) -> Result<Uri, ProxyError> {
        let backend_url = Url::parse(backend)
            .map_err(|e| ProxyError::InvalidUrl(format!("invalid backend URL: {}", e)))?;

        // Preserve original query string if present
        let mut final_url = backend_url;
        if let Some(query) = orig.query() {
            final_url.set_query(Some(query));
        }

        // Convert back to http::Uri
        final_url.as_str().parse()
            .map_err(|e| ProxyError::InvalidUrl(format!("failed to parse final URI: {}", e)))
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
    fn test_process_backend_url_with_params() {
        let mut params = HashMap::new();
        params.insert("id".to_string(), "123".to_string());
        params.insert("any".to_string(), "users/123".to_string());
        
        // Test basic parameter replacement
        let backend = "http://backend:8080/api/users/{id}";
        let result = ProxyHandler::process_backend_url_with_params(backend, &params).unwrap();
        assert_eq!(result, "http://backend:8080/api/users/123");
        
        // Test wildcard parameter replacement
        let backend2 = "http://zerex222.ru:8080/{*any}";
        let result2 = ProxyHandler::process_backend_url_with_params(backend2, &params).unwrap();
        assert_eq!(result2, "http://zerex222.ru:8080/users/123");
        
        // Test regex group parameters (like {1}, {2})
        let mut regex_params = HashMap::new();
        regex_params.insert("1".to_string(), "my".to_string());
        
        let backend3 = "http://zerex222.ru:8080/{1}";
        let result3 = ProxyHandler::process_backend_url_with_params(backend3, &regex_params).unwrap();
        assert_eq!(result3, "http://zerex222.ru:8080/my");
    }
    
    #[test]
    fn test_build_upstream_url() {
        let backend = "http://backend:8080/api/users/123";
        let orig_uri = "/api/users/123?active=true".parse::<Uri>().unwrap();
        
        let result = ProxyHandler::build_upstream_url(backend, &orig_uri).unwrap();
        assert_eq!(result.to_string(), "http://backend:8080/api/users/123?active=true");
    }
    
    #[test]
    fn test_build_upstream_url_user_case() {
        // Test case from user: path "/ip" should proxy to "http://zerex222.ru:8080/ip"
        let backend = "http://zerex222.ru:8080/ip";
        let orig_uri = "/ip".parse::<Uri>().unwrap();
        
        let result = ProxyHandler::build_upstream_url(backend, &orig_uri).unwrap();
        assert_eq!(result.to_string(), "http://zerex222.ru:8080/ip");
        
        // Test case: backend already contains full path
        let backend2 = "http://zerex222.ru:8080/ip/ololo";
        let orig_uri2 = "/ip/ololo".parse::<Uri>().unwrap();
        let result2 = ProxyHandler::build_upstream_url(backend2, &orig_uri2).unwrap();
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
    
    #[test]
    fn test_full_matcher_functionality() {
        // Test the example from user: "/my/{*any}" -> "http://zerex222.ru:8080/{*any}"
        let mut params = HashMap::new();
        params.insert("any".to_string(), "test/path".to_string());
        
        let backend = "http://zerex222.ru:8080/{*any}";
        let result = ProxyHandler::process_backend_url_with_params(backend, &params).unwrap();
        assert_eq!(result, "http://zerex222.ru:8080/test/path");
        
        // Test multiple parameters
        let mut params2 = HashMap::new();
        params2.insert("user".to_string(), "john".to_string());
        params2.insert("id".to_string(), "123".to_string());
        
        let backend2 = "http://backend:8080/users/{user}/posts/{id}";
        let result2 = ProxyHandler::process_backend_url_with_params(backend2, &params2).unwrap();
        assert_eq!(result2, "http://backend:8080/users/john/posts/123");
    }
}
