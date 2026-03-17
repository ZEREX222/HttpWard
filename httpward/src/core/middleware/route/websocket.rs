use rama::{
    http::{Request as RamaRequest, Response as RamaResponse, Body as RamaBody, header, HeaderMap},
};
use http::Response;
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum WebSocketError {
    #[error("upgrade not available")]
    UpgradeNotAvailable,
    #[error("invalid websocket URL: {0}")]
    InvalidUrl(String),
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// WebSocket proxy handler
#[derive(Clone, Debug)]
pub struct WebSocketHandler;

impl WebSocketHandler {
    pub fn new() -> Self {
        Self
    }
    
    /// Proxy WebSocket connection to upstream
    /// If httpward_headers is provided, they will be used instead of request headers (allowing middleware to modify them)
    pub async fn proxy_websocket(
        &self,
        req: RamaRequest<RamaBody>,
        _upstream_ws_url: &str,
        httpward_headers: Option<&HeaderMap>,
    ) -> Result<RamaResponse<RamaBody>, WebSocketError> {
        // TODO: Implement actual WebSocket upgrade handling
        // For now, return a basic 101 response to indicate upgrade acceptance
        let headers = httpward_headers.unwrap_or_else(|| req.headers());
        let resp = Response::builder()
            .status(101)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Accept", self.generate_accept_key(headers))
            .body(RamaBody::empty())
            .map_err(|e| WebSocketError::ConnectionFailed(format!("Failed to build response: {}", e)))?;
            
        Ok(resp)
    }
    
    /// Generate WebSocket accept key from request headers
    fn generate_accept_key(&self, headers: &HeaderMap) -> String {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use sha1::Digest;
        
        if let Some(key) = headers.get("Sec-WebSocket-Key") {
            if let Ok(key_str) = key.to_str() {
                let key_bytes = key_str.as_bytes();
                let mut hasher = sha1::Sha1::new();
                hasher.update(key_bytes);
                hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
                let result = hasher.finalize();
                return STANDARD.encode(result);
            }
        }
        
        // Fallback - this shouldn't happen in normal WebSocket handshake
        "dGhlIHNhbXBsZSBub25jZQ==".to_string() // "the sample nonce"
    }
    
    /// Convert HTTP URL to WebSocket URL
    pub fn http_to_ws_url(http_url: &str) -> Result<String, WebSocketError> {
        let url = Url::parse(http_url)
            .map_err(|e| WebSocketError::InvalidUrl(format!("Invalid URL: {}", e)))?;
            
        let mut ws_url = url.clone();
        
        match url.scheme() {
            "http" => ws_url.set_scheme("ws").unwrap(),
            "https" => ws_url.set_scheme("wss").unwrap(),
            "ws" | "wss" => {}, // Already WebSocket scheme
            _ => return Err(WebSocketError::InvalidUrl(
                format!("Unsupported scheme: {}", url.scheme())
            )),
        }
        
        Ok(ws_url.into())
    }
    
    /// Check if request is a WebSocket upgrade request
    pub fn is_websocket_request(req: &RamaRequest<RamaBody>) -> bool {
        // Check Upgrade header
        if let Some(upgrade) = req.headers().get(header::UPGRADE) {
            if let Ok(upgrade_str) = upgrade.to_str() {
                if upgrade_str.to_ascii_lowercase() == "websocket" {
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
    
    /// Check required WebSocket headers
    pub fn has_required_headers(req: &RamaRequest<RamaBody>) -> bool {
        let headers = req.headers();
        
        // Required headers for WebSocket upgrade
        headers.contains_key("Sec-WebSocket-Key") &&
        headers.contains_key("Sec-WebSocket-Version") &&
        Self::is_websocket_request(req)
    }
}

impl Default for WebSocketHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rama::http::{Method, Uri};
    
    #[test]
    fn test_http_to_ws_url() {
        assert_eq!(
            WebSocketHandler::http_to_ws_url("http://example.com").unwrap(),
            "ws://example.com/"
        );
        
        assert_eq!(
            WebSocketHandler::http_to_ws_url("https://example.com").unwrap(),
            "wss://example.com/"
        );
        
        assert_eq!(
            WebSocketHandler::http_to_ws_url("ws://example.com").unwrap(),
            "ws://example.com/"
        );
    }
    
    #[test]
    fn test_websocket_detection() {
        let mut req = RamaRequest::builder()
            .method(Method::GET)
            .uri("/ws")
            .header("Upgrade", "websocket")
            .header("Connection", "upgrade")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13")
            .body(RamaBody::empty())
            .unwrap();
            
        assert!(WebSocketHandler::is_websocket_request(&req));
        assert!(WebSocketHandler::has_required_headers(&req));
        
        // Test missing required headers
        let req2 = RamaRequest::builder()
            .method(Method::GET)
            .uri("/ws")
            .header("Upgrade", "websocket")
            .header("Connection", "upgrade")
            .body(RamaBody::empty())
            .unwrap();
            
        assert!(WebSocketHandler::is_websocket_request(&req2));
        assert!(!WebSocketHandler::has_required_headers(&req2));
    }
    
    #[test]
    fn test_generate_accept_key() {
        let handler = WebSocketHandler::new();
        let mut req = RamaRequest::builder()
            .method(Method::GET)
            .uri("/ws")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .body(RamaBody::empty())
            .unwrap();
            
        let accept_key = handler.generate_accept_key(req.headers());
        assert_eq!(accept_key, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }
}
