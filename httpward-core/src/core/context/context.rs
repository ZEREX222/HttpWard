use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use crate::config::{SiteConfig};
use crate::core::server_models::server_instance::ServerInstance;
use rama::http::headers::ContentType;



#[derive(Debug, Clone)]
pub struct HttpWardContext {
    pub client_ip: IpAddr,
    pub score: u32,
    pub site: Option<Arc<SiteConfig>>,
    pub server_instance: Arc<ServerInstance>,
    pub request_content_type: ContentType,
    pub response_content_type: ContentType,
    pub header_fp: Option<String>,
    pub ja4_fp: Option<String>
}

impl HttpWardContext {
    pub fn new(client_addr: SocketAddr, server_instance: Arc<ServerInstance>) -> Self {
        let client_ip = client_addr.ip();
        Self {
            client_ip,
            score: 0,
            request_content_type: ContentType::text(),
            response_content_type: ContentType::text(),
            site: None,
            server_instance,
            header_fp: None,
            ja4_fp: None
        }
    }
}
