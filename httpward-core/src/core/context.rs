use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use crate::config::{GlobalConfig, SiteConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    // --- Document Types (Often modified/injected) ---
    Html,           // text/html
    Xml,            // application/xml, text/xml
    PlainText,      // text/plain

    // --- Data & API Types ---
    Json,           // application/json
    Grpc,           // application/grpc (High-performance APIs)
    FormUrlEncoded, // application/x-www-form-urlencoded
    Multipart,      // multipart/form-data (File uploads)

    // --- Static Assets (Cacheable) ---
    JavaScript,     // text/javascript, application/javascript
    Css,            // text/css
    Image,          // image/png, image/jpeg, image/webp, image/gif
    Font,           // font/woff2, application/font-woff
    Video,          // video/mp4, video/webm

    // --- Application & Binary ---
    Pdf,            // application/pdf
    OctetStream,    // application/octet-stream (Generic binary)

    // --- Real-time & Streaming (Require special handling) ---
    EventStream,    // text/event-stream (SSE - Server Sent Events)
    Websocket,      // Connection: Upgrade (Tunneling)

    // --- Fallback ---
    Unknown,
}

#[derive(Debug, Clone)]
pub struct HttpWardContext {
    pub client_ip: IpAddr,
    pub score: u32,
    pub site: Option<Arc<SiteConfig>>,
    pub global: Arc<GlobalConfig>,
    pub request_content_type: ContentType,
    pub response_content_type: ContentType,
    pub header_fp: Option<String>,
    pub ja4_fp: Option<String>
}

impl HttpWardContext {
    pub fn new(client_addr: SocketAddr, global: Arc<GlobalConfig>) -> Self {
        let client_ip = client_addr.ip();
        Self {
            client_ip,
            score: 0,
            request_content_type: ContentType::Unknown,
            response_content_type: ContentType::Unknown,
            site: None,
            global,
            header_fp: None,
            ja4_fp: None
        }
    }
}
