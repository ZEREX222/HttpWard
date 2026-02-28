use std::net::SocketAddr;

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

pub struct RequestContext {
    pub client_addr: SocketAddr,
    pub score: u32,
    pub content_type: ContentType,
    pub fingerprint: Option<String>,
}

impl RequestContext {
    pub fn new(client_addr: SocketAddr) -> Self {
        Self {
            client_addr,
            score: 0,
            content_type: ContentType::Unknown,
            fingerprint: None,
        }
    }
}
