use rama::{
    http::{Request as RamaRequest, Response as RamaResponse, Body as RamaBody, header, HeaderValue, Version},
    http::client::EasyHttpWebClient,
    http::service::client::HttpClientExt,
};
use http::{HeaderMap, HeaderName, Uri};
use thiserror::Error;
use url::Url;
use std::collections::{HashMap, HashSet};

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("http client error: {0}")]
    Http(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("upstream error: {0}")]
    Upstream(String),
}

/// Remove hop-by-hop headers and headers listed in Connection, then
/// add/append forwarding headers (Via, X-Forwarded-For, etc).
fn normalize_request_headers(
    mut headers: HeaderMap,
    client_ip: Option<&str>,
    upstream_host: &str,
    incoming_proto: &str, // "http" or "https"
) -> Result<HeaderMap, Box<dyn std::error::Error>> {
    // Hop-by-hop headers per RFC: always remove these.
    let mut hop_by_hop = vec![
        header::CONNECTION.clone(),
        header::KEEP_ALIVE.clone(),
        header::PROXY_AUTHENTICATE.clone(),
        header::PROXY_AUTHORIZATION.clone(),
        header::TE.clone(),
        header::TRAILER.clone(),
        header::TRANSFER_ENCODING.clone(),
        header::UPGRADE.clone(),
    ].into_iter().collect::<HashSet<_>>();

    // If Connection header exists, its comma-separated tokens name additional hop-by-hop headers.
    if let Some(conn_val) = headers.get(header::CONNECTION) {
        if let Ok(s) = conn_val.to_str() {
            for token in s.split(',').map(|t| t.trim()) {
                if !token.is_empty() {
                    if let Ok(hname) = HeaderName::from_lowercase(token.to_lowercase().as_bytes()) {
                        hop_by_hop.insert(hname);
                    }
                }
            }
        }
    }

    // Remove hop-by-hop headers
    for name in hop_by_hop {
        headers.remove(name);
    }

    // Ensure Host header equals upstream authority
    headers.insert(
        header::HOST,
        HeaderValue::from_str(upstream_host)?,
    );

    // Preserve original host in X-Forwarded-Host if present (optional)
    // You can add logic to set X-Forwarded-Host only if original Host != upstream_host.
    // headers.insert(HeaderName::from_static("x-forwarded-host"), ...);

    // Append or create X-Forwarded-For
    if let Some(ip) = client_ip {
        let xff = HeaderName::from_static("x-forwarded-for");
        let prev = headers.get(&xff).and_then(|v| v.to_str().ok()).unwrap_or("");
        let new_val = if prev.is_empty() {
            ip.to_string()
        } else {
            format!("{}, {}", prev, ip)
        };
        headers.insert(xff, HeaderValue::from_str(&new_val)?);
    }

    // Add X-Forwarded-Proto
    headers.insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_str(incoming_proto)?,
    );

    // Add Via header
    let via_name = HeaderName::from_static("via");
    let our_via = format!("{} {}", incoming_proto, "my-proxy"); // replace my-proxy with actual id
    let via_prev = headers.get(&via_name).and_then(|v| v.to_str().ok()).unwrap_or("");
    let via_val = if via_prev.is_empty() {
        our_via
    } else {
        format!("{}, {}", via_prev, our_via)
    };
    headers.insert(via_name, HeaderValue::from_str(&via_val)?);

    Ok(headers)
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
    /// If httpward_headers is provided, they will be used instead of request headers (allowing middleware to modify them)
    pub async fn proxy_request(
        &self,
        mut req: RamaRequest<RamaBody>,
        backend: &str,
        params: &HashMap<String, String>,
        httpward_headers: Option<HeaderMap>,
    ) -> Result<RamaResponse<RamaBody>, ProxyError> {
        // Process backend URL with matcher parameters
        let processed_backend = Self::process_backend_url_with_params(backend, params)?;
        
        // Build upstream URL
        let new_uri = Self::build_upstream_url(&processed_backend, req.uri())?;
        *req.uri_mut() = new_uri;

        // Use headers from HttpWardContext if provided (allows middleware modifications)
        let mut headers = if let Some(ctx_headers) = httpward_headers {
            ctx_headers
        } else {
            req.headers().clone()
        };

        // Ensure Host header for upstream
        let authority = req.uri().authority().map(|a| a.to_string());
        if !headers.contains_key(header::HOST) {
            if let Some(authority_str) = authority {
                headers.insert(
                    header::HOST,
                    HeaderValue::from_str(&authority_str)
                        .map_err(|e| ProxyError::Upstream(format!("Invalid host header: {}", e)))?,
                );
            }
        }

        // Extract upstream authority (host:port)
        let upstream_host = req
            .uri()
            .authority()
            .map(|a| a.as_str())
            .ok_or_else(|| ProxyError::Upstream("Missing upstream authority".into()))?;

        // Example values (normally from connection context)
        let client_ip = None; // Option<&str>
        let proto = "http";   // or "https"

        // Normalize headers before sending upstream
        let normalized_headers = normalize_request_headers(
            headers,
            client_ip,
            upstream_host,
            proto,
        ).map_err(|e| ProxyError::Upstream(e.to_string()))?;

        // Send request using Rama HTTP client
        let resp = self.client
            .request(req.method().clone(), req.uri().clone())
            .headers(normalized_headers)
            .body(req.into_body())
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
