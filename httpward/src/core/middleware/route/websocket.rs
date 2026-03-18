use futures_util::{SinkExt, StreamExt};
use rama::{
    Context,
    http::{
        Body as RamaBody, HeaderMap, Request as RamaRequest, Response as RamaResponse,
        StatusCode, header,
    },
};
use http::{Method, Request as HttpRequest, Response};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, connect_async};
use tokio_tungstenite::tungstenite::{Error as TungsteniteError, protocol::Role};
use tracing::error;
use url::Url;

#[derive(Error, Debug)]
pub enum WebSocketError {
    #[error("invalid websocket request: {0}")]
    InvalidRequest(String),
    #[error("invalid websocket URL: {0}")]
    InvalidUrl(String),
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("websocket error: {0}")]
    WebSocket(#[from] TungsteniteError),
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
    pub async fn proxy_websocket<State>(
        &self,
        ctx: &Context<State>,
        mut req: RamaRequest<RamaBody>,
        upstream_ws_url: &str,
        httpward_headers: Option<&HeaderMap>,
    ) -> Result<RamaResponse<RamaBody>, WebSocketError>
    where
        State: Clone + Send + Sync + 'static,
    {
        if req.method() != Method::GET {
            return Err(WebSocketError::InvalidRequest(
                "websocket upgrade requires GET".to_string(),
            ));
        }

        let effective_headers = httpward_headers
            .cloned()
            .unwrap_or_else(|| req.headers().clone());

        if !Self::has_required_headers_in_map(&effective_headers) {
            return Err(WebSocketError::InvalidRequest(
                "missing required websocket upgrade headers".to_string(),
            ));
        }

        let accept_key = self.generate_accept_key(&effective_headers)?;
        let upstream_request = self.build_upstream_request(upstream_ws_url, &effective_headers)?;
        let (upstream_ws, upstream_response) = connect_async(upstream_request)
            .await
            .map_err(|e| WebSocketError::ConnectionFailed(e.to_string()))?;

        let selected_protocol = upstream_response
            .headers()
            .get("Sec-WebSocket-Protocol")
            .cloned();

        let on_upgrade = rama::http::core::upgrade::on(&mut req);
        ctx.spawn(async move {
            match on_upgrade.await {
                Ok(upgraded) => {
                    let client_ws = WebSocketStream::from_raw_socket(upgraded, Role::Server, None).await;
                    if let Err(err) = Self::relay_websocket_streams(client_ws, upstream_ws).await {
                        error!(error = %err, "websocket proxy relay failed");
                    }
                }
                Err(err) => {
                    error!(error = %err, "websocket client upgrade failed");
                }
            }
        });

        let mut resp = Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Accept", accept_key);

        if let Some(protocol) = selected_protocol {
            resp = resp.header("Sec-WebSocket-Protocol", protocol);
        }

        let resp = resp
            .body(RamaBody::empty())
            .map_err(|e| WebSocketError::ConnectionFailed(format!("Failed to build response: {}", e)))?;

        Ok(resp)
    }
    
    /// Generate WebSocket accept key from request headers
    fn generate_accept_key(&self, headers: &HeaderMap) -> Result<String, WebSocketError> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use sha1::Digest;
        
        if let Some(key) = headers.get("Sec-WebSocket-Key") {
            if let Ok(key_str) = key.to_str() {
                let key_bytes = key_str.as_bytes();
                let mut hasher = sha1::Sha1::new();
                hasher.update(key_bytes);
                hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
                let result = hasher.finalize();
                return Ok(STANDARD.encode(result));
            }
        }

        Err(WebSocketError::InvalidRequest(
            "missing or invalid Sec-WebSocket-Key".to_string(),
        ))
    }

    fn build_upstream_request(
        &self,
        upstream_ws_url: &str,
        headers: &HeaderMap,
    ) -> Result<HttpRequest<()>, WebSocketError> {
        let url = Url::parse(upstream_ws_url)
            .map_err(|e| WebSocketError::InvalidUrl(format!("Invalid URL: {}", e)))?;

        let host = url
            .host_str()
            .ok_or_else(|| WebSocketError::InvalidUrl("missing websocket host".to_string()))?;
        let authority = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };

        let mut upstream_request = HttpRequest::builder()
            .method(Method::GET)
            .uri(upstream_ws_url)
            .body(())
            .map_err(|e| WebSocketError::ConnectionFailed(format!("Failed to build upstream request: {}", e)))?;

        for (name, value) in headers {
            if Self::should_forward_upstream_header(name.as_str()) {
                upstream_request.headers_mut().insert(name.clone(), value.clone());
            }
        }

        upstream_request.headers_mut().insert(
            header::HOST,
            authority
                .parse()
                .map_err(|e| WebSocketError::InvalidRequest(format!("invalid upstream host header: {}", e)))?,
        );
        upstream_request.headers_mut().insert(
            header::CONNECTION,
            "Upgrade"
                .parse()
                .map_err(|e| WebSocketError::InvalidRequest(format!("invalid connection header: {}", e)))?,
        );
        upstream_request.headers_mut().insert(
            header::UPGRADE,
            "websocket"
                .parse()
                .map_err(|e| WebSocketError::InvalidRequest(format!("invalid upgrade header: {}", e)))?,
        );

        if !Self::has_required_headers_in_map(upstream_request.headers()) {
            return Err(WebSocketError::InvalidRequest(
                "upstream websocket request is missing required headers".to_string(),
            ));
        }

        Ok(upstream_request)
    }

    fn should_forward_upstream_header(name: &str) -> bool {
        !matches!(
            name.to_ascii_lowercase().as_str(),
            "host"
                | "connection"
                | "upgrade"
                | "sec-websocket-accept"
                | "sec-websocket-extensions"
                | "content-length"
        )
    }

    async fn relay_websocket_streams<ClientStream, UpstreamStream>(
        client_ws: WebSocketStream<ClientStream>,
        upstream_ws: WebSocketStream<UpstreamStream>,
    ) -> Result<(), WebSocketError>
    where
        ClientStream: AsyncRead + AsyncWrite + Unpin + Send + 'static,
        UpstreamStream: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (mut client_sink, mut client_stream) = client_ws.split();
        let (mut upstream_sink, mut upstream_stream) = upstream_ws.split();

        let client_to_upstream = async {
            while let Some(message) = client_stream.next().await {
                upstream_sink.send(message?).await?;
            }
            upstream_sink.close().await?;
            Result::<(), TungsteniteError>::Ok(())
        };

        let upstream_to_client = async {
            while let Some(message) = upstream_stream.next().await {
                client_sink.send(message?).await?;
            }
            client_sink.close().await?;
            Result::<(), TungsteniteError>::Ok(())
        };

        match tokio::try_join!(client_to_upstream, upstream_to_client) {
            Ok(((), ())) => Ok(()),
            Err(TungsteniteError::ConnectionClosed | TungsteniteError::AlreadyClosed) => Ok(()),
            Err(err) => Err(WebSocketError::WebSocket(err)),
        }
    }

    fn has_required_headers_in_map(headers: &HeaderMap) -> bool {
        Self::is_websocket_headers(headers)
            && headers.contains_key("Sec-WebSocket-Key")
            && headers.contains_key("Sec-WebSocket-Version")
    }

    fn is_websocket_headers(headers: &HeaderMap) -> bool {
        if let Some(upgrade) = headers.get(header::UPGRADE) {
            if let Ok(upgrade_str) = upgrade.to_str() {
                if upgrade_str.eq_ignore_ascii_case("websocket") {
                    if let Some(connection) = headers.get(header::CONNECTION) {
                        if let Ok(conn_str) = connection.to_str() {
                            return conn_str.to_ascii_lowercase().contains("upgrade");
                        }
                    }
                }
            }
        }
        false
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
        Self::is_websocket_headers(req.headers())
    }
    
    /// Check required WebSocket headers
    pub fn has_required_headers(req: &RamaRequest<RamaBody>) -> bool {
        Self::has_required_headers_in_map(req.headers())
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
    use tokio::io::duplex;
    use tokio_tungstenite::tungstenite::Message;

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
        let req = RamaRequest::builder()
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
        let req = RamaRequest::builder()
            .method(Method::GET)
            .uri("/ws")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .body(RamaBody::empty())
            .unwrap();
            
        let accept_key = handler.generate_accept_key(req.headers()).unwrap();
        assert_eq!(accept_key, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn test_build_upstream_request_preserves_important_headers() {
        let handler = WebSocketHandler::new();
        let req = RamaRequest::builder()
            .method(Method::GET)
            .uri("/ws?token=1")
            .header("Host", "frontend.local")
            .header("Upgrade", "websocket")
            .header("Connection", "keep-alive, Upgrade")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Protocol", "chat, superchat")
            .header("Sec-WebSocket-Extensions", "permessage-deflate")
            .header("Origin", "https://frontend.local")
            .header("Cookie", "session=abc")
            .body(RamaBody::empty())
            .unwrap();

        let upstream_request = handler
            .build_upstream_request("ws://backend.internal:9001/socket?token=1", req.headers())
            .unwrap();

        assert_eq!(upstream_request.uri(), "ws://backend.internal:9001/socket?token=1");
        assert_eq!(upstream_request.headers()[header::HOST], "backend.internal:9001");
        assert_eq!(upstream_request.headers()[header::UPGRADE], "websocket");
        assert_eq!(upstream_request.headers()["sec-websocket-protocol"], "chat, superchat");
        assert_eq!(upstream_request.headers()["origin"], "https://frontend.local");
        assert_eq!(upstream_request.headers()["cookie"], "session=abc");
        assert!(!upstream_request.headers().contains_key("Sec-WebSocket-Extensions"));
    }

    #[tokio::test]
    async fn test_relay_websocket_streams_bidirectional() {
        let (proxy_client_io, mut client_peer_io) = duplex(4096);
        let (proxy_upstream_io, mut upstream_peer_io) = duplex(4096);

        let proxy_client_ws = WebSocketStream::from_raw_socket(proxy_client_io, Role::Server, None).await;
        let proxy_upstream_ws = WebSocketStream::from_raw_socket(proxy_upstream_io, Role::Client, None).await;

        let relay_task = tokio::spawn(async move {
            WebSocketHandler::relay_websocket_streams(proxy_client_ws, proxy_upstream_ws)
                .await
                .unwrap();
        });

        let mut client_peer_ws = WebSocketStream::from_raw_socket(&mut client_peer_io, Role::Client, None).await;
        let mut upstream_peer_ws = WebSocketStream::from_raw_socket(&mut upstream_peer_io, Role::Server, None).await;

        client_peer_ws
            .send(Message::Text("hello-upstream".into()))
            .await
            .unwrap();
        let upstream_message = upstream_peer_ws.next().await.unwrap().unwrap();
        assert_eq!(upstream_message.into_text().unwrap(), "hello-upstream");

        upstream_peer_ws
            .send(Message::Binary(vec![1_u8, 2, 3].into()))
            .await
            .unwrap();
        let client_message = client_peer_ws.next().await.unwrap().unwrap();
        assert_eq!(client_message.into_data(), vec![1_u8, 2, 3]);

        drop(client_peer_ws);
        drop(upstream_peer_ws);
        relay_task.abort();
    }
}
