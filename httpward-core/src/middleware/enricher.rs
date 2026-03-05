use rama::{
    http::{Request, HeaderMap},
    layer::Layer,
    service::Service,
    Context,
};
use std::fmt::Debug;
use std::sync::Arc;
use tracing::trace;
use wildmatch::WildMatch;

use crate::middleware::core::{ContentType, HttpWardContext};
use crate::config::{SiteConfig, GlobalConfig};
use rama::http::Body;

/// Layer that enriches request context with HttpWardContext containing client_addr, content_type, site and global configs
#[derive(Clone, Debug)]
pub struct EnricherLayer {
    sites: Vec<Arc<SiteConfig>>,
    global: Arc<GlobalConfig>,
}

impl EnricherLayer {
    pub fn new(sites: Vec<Arc<SiteConfig>>, global: Arc<GlobalConfig>) -> Self {
        Self { sites, global }
    }
}

impl<S> Layer<S> for EnricherLayer {
    type Service = EnricherService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        EnricherService::new(inner, self.sites.clone(), self.global.clone())
    }
}

/// Service that enriches requests with HttpWardContext
#[derive(Clone, Debug)]
pub struct EnricherService<S> {
    inner: S,
    global: Arc<GlobalConfig>,
    // Cached WildMatch patterns for better performance
    cached_patterns: Vec<(Arc<SiteConfig>, Vec<WildMatch>)>,
}

impl<S> EnricherService<S> {
    pub fn new(inner: S, sites: Vec<Arc<SiteConfig>>, global: Arc<GlobalConfig>) -> Self {
        // Pre-cache WildMatch patterns for all domains
        let cached_patterns = sites.iter().map(|site| {
            let all_domains = site.get_all_domains();
            let patterns: Vec<WildMatch> = all_domains
                .iter()
                .map(|domain| WildMatch::new(domain))
                .collect();
            (site.clone(), patterns)
        }).collect();

        Self { 
            inner, 
            global,
            cached_patterns,
        }
    }

    /// Find site configuration by SNI from TLS context
    fn find_site_by_sni<State>(&self, ctx: &Context<State>) -> Option<Arc<SiteConfig>> {
        // Try to get SNI from TLS context
        if let Some(st) = ctx.get::<rama::net::tls::SecureTransport>() {
            if let Some(client_hello) = st.client_hello() {
                if let Some(sni) = client_hello.ext_server_name() {
                    let sni_str = sni.to_string();
                    
                    // Find site using cached WildMatch patterns (more efficient)
                    return self.cached_patterns.iter().find(|(_, patterns)| {
                        patterns.iter().any(|pattern| pattern.matches(&sni_str))
                    }).map(|(site, _)| site.clone());
                }
            }
        }
        None
    }
}

impl<S, State> Service<State, Request<Body>> for EnricherService<S>
where
    S: Service<State, Request<Body>>,
    S::Response: Debug,
    S::Error: Debug,
    State: Clone + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = S::Error;

    async fn serve(
        &self,
        mut ctx: Context<State>,
        request: Request<Body>,
    ) -> Result<Self::Response, Self::Error> {
        // Extract client address from context if available
        let client_addr = ctx.get::<rama::net::address::SocketAddress>()
            .map(|addr| std::net::SocketAddr::new(*addr.ip_addr(), 0))
            .or_else(|| ctx.get::<std::net::SocketAddr>().copied())
            .unwrap_or_else(|| std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)), 0));

        // Extract content type from request headers
        let content_type = extract_content_type_from_request(&request);

        // Find site by SNI from TLS context if available
        let site = self.find_site_by_sni(&ctx);
        let site_domain = site.as_ref().map(|s| s.domain.as_str());

        // Create and insert HttpWardContext into the context
        let enriched_context = HttpWardContext {
            client_addr,
            content_type,
            score: 0,
            site: site.clone(),
            global: self.global.clone(),
        };

        ctx.insert(enriched_context);

        trace!("enriched request with HttpWardContext: client_addr={}, content_type={:?}, site={:?}", 
               client_addr, content_type, site_domain);

        // Continue with the inner service
        self.inner.serve(ctx, request).await
    }
}

/// Extract content type from request headers
fn extract_content_type_from_request(request: &Request<Body>) -> ContentType {
    // Try to extract headers from the request
    if let Some(headers) = extract_headers_from_request(request) {
        if let Some(content_type_header) = headers.get("content-type") {
            if let Ok(content_type_str) = content_type_header.to_str() {
                return parse_content_type(content_type_str);
            }
        }
    }
    ContentType::Unknown
}

/// Extract headers from request - specialized for HTTP requests
fn extract_headers_from_request(request: &Request<Body>) -> Option<&HeaderMap> {
    Some(request.headers())
}

/// Parse content type string into ContentType enum
fn parse_content_type(content_type_str: &str) -> ContentType {
    let content_type_str = content_type_str.to_lowercase();
    
    if content_type_str.contains("text/html") {
        ContentType::Html
    } else if content_type_str.contains("application/json") {
        ContentType::Json
    } else if content_type_str.contains("application/xml") || content_type_str.contains("text/xml") {
        ContentType::Xml
    } else if content_type_str.contains("text/plain") {
        ContentType::PlainText
    } else if content_type_str.contains("text/css") {
        ContentType::Css
    } else if content_type_str.contains("application/javascript") || content_type_str.contains("text/javascript") {
        ContentType::JavaScript
    } else if content_type_str.contains("image/") {
        ContentType::Image
    } else if content_type_str.contains("video/") {
        ContentType::Video
    } else if content_type_str.contains("application/pdf") {
        ContentType::Pdf
    } else if content_type_str.contains("application/grpc") {
        ContentType::Grpc
    } else if content_type_str.contains("application/x-www-form-urlencoded") {
        ContentType::FormUrlEncoded
    } else if content_type_str.contains("multipart/form-data") {
        ContentType::Multipart
    } else if content_type_str.contains("application/octet-stream") {
        ContentType::OctetStream
    } else if content_type_str.contains("text/event-stream") {
        ContentType::EventStream
    } else if content_type_str.contains("font/") {
        ContentType::Font
    } else {
        ContentType::Unknown
    }
}
