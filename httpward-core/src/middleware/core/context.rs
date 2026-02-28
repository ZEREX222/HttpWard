use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub enum ContentType {
    Html,
    Json,
    Static,
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
