use http::{HeaderMap, HeaderName, Uri};
use rama::{
    http::client::EasyHttpWebClient,
    http::service::client::HttpClientExt,
    http::{
        Body as RamaBody, HeaderValue, Request as RamaRequest, Response as RamaResponse, header,
    },
};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProxyRequestKind {
    Http,
    Grpc,
}

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
    request_kind: ProxyRequestKind,
    proxy_id: &str,
) -> Result<HeaderMap, Box<dyn std::error::Error>> {
    // Preserve original Host header before replacing it
    if let Some(original_host) = headers.get(header::HOST) {
        if let Ok(host_str) = original_host.to_str() {
            // Only add X-Forwarded-Host if it's different from upstream host
            if host_str != upstream_host {
                headers.insert(
                    HeaderName::from_static("x-forwarded-host"),
                    HeaderValue::from_str(host_str)?,
                );
            }
        }
    }

    // Hop-by-hop headers per RFC: always remove these.
    let mut hop_by_hop = vec![
        header::CONNECTION.clone(),
        header::KEEP_ALIVE.clone(),
        header::PROXY_AUTHENTICATE.clone(),
        header::PROXY_AUTHORIZATION.clone(),
        header::TRAILER.clone(),
        header::TRANSFER_ENCODING.clone(),
        header::UPGRADE.clone(),
    ]
    .into_iter()
    .collect::<HashSet<_>>();

    if request_kind == ProxyRequestKind::Http {
        hop_by_hop.insert(header::TE.clone());
    }

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

    // gRPC over HTTP/2 relies on TE: trailers.
    if request_kind == ProxyRequestKind::Grpc {
        headers.insert(header::TE, HeaderValue::from_static("trailers"));
    }

    // Ensure Host header equals upstream authority
    headers.insert(header::HOST, HeaderValue::from_str(upstream_host)?);

    // Append or create X-Forwarded-For with IPv6 support
    if let Some(ip) = client_ip {
        let xff = HeaderName::from_static("x-forwarded-for");
        let prev = headers
            .get(&xff)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // Validate IP format (supports both IPv4 and IPv6)
        let normalized_ip = if ip.contains(':') && ip.starts_with('[') && ip.ends_with(']') {
            // IPv6 in brackets [::1] -> ::1
            ip.trim_start_matches('[').trim_end_matches(']')
        } else {
            ip // IPv4 or IPv6 without brackets
        };

        let new_val = if prev.is_empty() {
            normalized_ip.to_string()
        } else {
            format!("{}, {}", prev, normalized_ip)
        };
        headers.insert(xff, HeaderValue::from_str(&new_val)?);
    }

    // Add X-Forwarded-Proto
    headers.insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_str(incoming_proto)?,
    );

    // Add Via header with configurable proxy identifier
    let via_name = HeaderName::from_static("via");
    let our_via = format!("{} {}", incoming_proto, proxy_id);
    let via_prev = headers
        .get(&via_name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let via_val = if via_prev.is_empty() {
        our_via
    } else {
        format!("{}, {}", via_prev, our_via)
    };
    headers.insert(via_name, HeaderValue::from_str(&via_val)?);

    Ok(headers)
}

/// Remove hop-by-hop response headers and headers listed in Connection.
fn normalize_response_headers(mut headers: HeaderMap) -> HeaderMap {
    let mut hop_by_hop = vec![
        header::CONNECTION.clone(),
        header::KEEP_ALIVE.clone(),
        header::PROXY_AUTHENTICATE.clone(),
        header::PROXY_AUTHORIZATION.clone(),
        header::TE.clone(),
        header::TRAILER.clone(),
        header::TRANSFER_ENCODING.clone(),
        header::UPGRADE.clone(),
        HeaderName::from_static("proxy-connection"),
    ]
    .into_iter()
    .collect::<HashSet<_>>();

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

    for name in hop_by_hop {
        headers.remove(name);
    }

    headers
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

    pub(crate) fn build_proxy_uri(
        backend: &str,
        params: &HashMap<String, String>,
        orig: &Uri,
    ) -> Result<Uri, ProxyError> {
        let processed_backend = Self::process_backend_url_with_params(backend, params)?;
        Self::build_upstream_url(&processed_backend, orig)
    }

    /// Proxy HTTP request to upstream
    /// If httpward_headers is provided, they will be used instead of request headers (allowing middleware to modify them)
    pub async fn proxy_request(
        &self,
        req: RamaRequest<RamaBody>,
        backend: &str,
        params: &HashMap<String, String>,
        httpward_headers: Option<HeaderMap>,
    ) -> Result<RamaResponse<RamaBody>, ProxyError> {
        self.proxy_request_with_kind_and_client_ip(
            req,
            backend,
            params,
            httpward_headers,
            ProxyRequestKind::Http,
            "httpward",
            None,
        )
        .await
    }

    /// Proxy gRPC request to upstream preserving gRPC-required headers.
    pub async fn proxy_grpc_request(
        &self,
        req: RamaRequest<RamaBody>,
        backend: &str,
        params: &HashMap<String, String>,
        httpward_headers: Option<HeaderMap>,
    ) -> Result<RamaResponse<RamaBody>, ProxyError> {
        self.proxy_request_with_kind_and_client_ip(
            req,
            backend,
            params,
            httpward_headers,
            ProxyRequestKind::Grpc,
            "httpward",
            None,
        )
        .await
    }

    /// Proxy HTTP request to upstream with client IP and proxy ID
    /// If httpward_headers is provided, they will be used instead of request headers (allowing middleware to modify them)
    pub async fn proxy_request_with_client_ip_and_proxy_id(
        &self,
        req: RamaRequest<RamaBody>,
        backend: &str,
        params: &HashMap<String, String>,
        httpward_headers: Option<HeaderMap>,
        client_ip: Option<&str>,
        proxy_id: &str,
    ) -> Result<RamaResponse<RamaBody>, ProxyError> {
        self.proxy_request_with_kind_and_client_ip(
            req,
            backend,
            params,
            httpward_headers,
            ProxyRequestKind::Http,
            proxy_id,
            client_ip,
        )
        .await
    }

    async fn proxy_request_with_kind_and_client_ip(
        &self,
        mut req: RamaRequest<RamaBody>,
        backend: &str,
        params: &HashMap<String, String>,
        httpward_headers: Option<HeaderMap>,
        request_kind: ProxyRequestKind,
        proxy_id: &str,
        client_ip: Option<&str>,
    ) -> Result<RamaResponse<RamaBody>, ProxyError> {
        // Use headers from HttpWardContext if provided (allows middleware modifications)
        let headers = httpward_headers.unwrap_or_else(|| req.headers().clone());

        // Build upstream URL using matcher parameters and original request query
        let new_uri = Self::build_proxy_uri(backend, params, req.uri())?;
        *req.uri_mut() = new_uri;

        // Extract upstream authority and protocol from the already processed URI
        let upstream_host = req.uri().authority().map(|a| a.as_str()).ok_or_else(|| {
            ProxyError::InvalidUrl("Missing upstream authority in processed URI".to_string())
        })?;

        let proto = req.uri().scheme().map(|s| s.as_str()).unwrap_or("http");

        // Normalize headers before sending upstream
        let normalized_headers = normalize_request_headers(
            headers,
            client_ip,
            &upstream_host,
            proto,
            request_kind,
            proxy_id,
        )
        .map_err(|e| ProxyError::Upstream(e.to_string()))?;

        // Send request using Rama HTTP client
        let resp = self
            .client
            .request(req.method().clone(), req.uri().clone())
            .headers(normalized_headers)
            .body(req.into_body())
            .send(rama::Context::default())
            .await
            .map_err(|e| ProxyError::Http(Box::new(e)))?;

        let mut resp = resp;
        let normalized_response_headers = normalize_response_headers(resp.headers().clone());
        *resp.headers_mut() = normalized_response_headers;

        Ok(resp)
    }

    /// Build upstream URI by combining processed backend URL with original request
    fn build_upstream_url(backend: &str, orig: &Uri) -> Result<Uri, ProxyError> {
        let mut backend_url = Url::parse(backend)
            .map_err(|e| ProxyError::InvalidUrl(format!("invalid backend URL: {}", e)))?;

        // Preserve original path and query if backend doesn't specify them
        if backend_url.path().is_empty() || backend_url.path() == "/" {
            // Use original request path
            backend_url.set_path(orig.path());
        }

        // Preserve original query string if present and backend doesn't have one
        if let Some(query) = orig.query() {
            if backend_url.query().is_none() {
                backend_url.set_query(Some(query));
            }
        }

        // Convert back to http::Uri
        backend_url
            .as_str()
            .parse()
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
        // gRPC requests always use application/grpc* content types.
        if let Some(content_type) = req.headers().get(header::CONTENT_TYPE) {
            if let Ok(ct_str) = content_type.to_str() {
                if ct_str.starts_with("application/grpc") {
                    return true;
                }
            }
        }
        false
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
    use rama::http::{Method, Uri, Version};

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
        let result3 =
            ProxyHandler::process_backend_url_with_params(backend3, &regex_params).unwrap();
        assert_eq!(result3, "http://zerex222.ru:8080/my");
    }

    #[test]
    fn test_build_upstream_url() {
        let backend = "http://backend:8080/api/users/123";
        let orig_uri = "/api/users/123?active=true".parse::<Uri>().unwrap();

        let result = ProxyHandler::build_upstream_url(backend, &orig_uri).unwrap();
        assert_eq!(
            result.to_string(),
            "http://backend:8080/api/users/123?active=true"
        );
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
    fn test_grpc_detection_does_not_match_plain_http2() {
        let req = RamaRequest::builder()
            .method(Method::GET)
            .uri("/api")
            .version(Version::HTTP_2)
            .body(RamaBody::empty())
            .unwrap();

        assert!(!ProxyHandler::is_grpc(&req));
    }

    #[test]
    fn test_normalize_http_headers_removes_te() {
        let mut headers = HeaderMap::new();
        headers.insert(header::TE, HeaderValue::from_static("trailers"));

        let normalized = normalize_request_headers(
            headers,
            None,
            "backend.local:8080",
            "http",
            ProxyRequestKind::Http,
            "httpward",
        )
        .unwrap();

        assert!(!normalized.contains_key(header::TE));
    }

    #[test]
    fn test_normalize_grpc_headers_preserves_te_trailers() {
        let mut headers = HeaderMap::new();
        headers.insert(header::TE, HeaderValue::from_static("gzip, trailers"));

        let normalized = normalize_request_headers(
            headers,
            None,
            "backend.local:8080",
            "http",
            ProxyRequestKind::Grpc,
            "httpward",
        )
        .unwrap();

        assert_eq!(
            normalized.get(header::TE).unwrap(),
            &HeaderValue::from_static("trailers")
        );
    }

    #[test]
    fn test_normalize_response_headers_removes_hop_by_hop() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONNECTION,
            HeaderValue::from_static("keep-alive, x-hop"),
        );
        headers.insert(
            header::KEEP_ALIVE.clone(),
            HeaderValue::from_static("timeout=5"),
        );
        headers.insert(
            header::TRANSFER_ENCODING,
            HeaderValue::from_static("chunked"),
        );
        headers.insert(header::TRAILER, HeaderValue::from_static("x-trailer"));
        headers.insert(header::UPGRADE, HeaderValue::from_static("h2c"));
        headers.insert(header::TE, HeaderValue::from_static("trailers"));
        headers.insert(
            HeaderName::from_static("x-hop"),
            HeaderValue::from_static("1"),
        );

        let normalized = normalize_response_headers(headers);

        assert!(!normalized.contains_key(header::CONNECTION));
        assert!(!normalized.contains_key(header::KEEP_ALIVE.clone()));
        assert!(!normalized.contains_key(header::TRANSFER_ENCODING));
        assert!(!normalized.contains_key(header::TRAILER));
        assert!(!normalized.contains_key(header::UPGRADE));
        assert!(!normalized.contains_key(header::TE));
        assert!(!normalized.contains_key("x-hop"));
    }

    #[test]
    fn test_normalize_response_headers_preserves_grpc_metadata() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/grpc"),
        );
        headers.insert(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("0"),
        );
        headers.insert(
            HeaderName::from_static("grpc-message"),
            HeaderValue::from_static("ok"),
        );
        headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));

        let normalized = normalize_response_headers(headers);

        assert_eq!(
            normalized.get(header::CONTENT_TYPE).unwrap(),
            &HeaderValue::from_static("application/grpc")
        );
        assert_eq!(
            normalized.get("grpc-status").unwrap(),
            &HeaderValue::from_static("0")
        );
        assert_eq!(
            normalized.get("grpc-message").unwrap(),
            &HeaderValue::from_static("ok")
        );
        assert!(!normalized.contains_key(header::CONNECTION));
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
