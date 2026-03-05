use rama::{
    http::Request,
    layer::Layer,
    service::Service,
    Context,
};
use std::fmt::Debug;
use std::sync::Arc;
use tracing::trace;
use wildmatch::WildMatch;

use crate::middleware::core::{ContentType, HttpWardContext, parse_content_type};
use crate::config::{SiteConfig, GlobalConfig};
use rama::http::Body;

/// Extract content type from request headers
fn extract_content_type_from_request(request: &Request<Body>) -> ContentType {
    // Try to extract headers from the request
    if let Some(headers) = request.headers().get("content-type") {
        if let Ok(content_type_str) = headers.to_str() {
            return parse_content_type(content_type_str);
        }
    }
    ContentType::Unknown
}

/// Layer that enriches request context with HttpWardContext containing client_addr, site and global configs
#[derive(Clone, Debug)]
pub struct RequestEnricherLayer {
    sites: Vec<Arc<SiteConfig>>,
    global: Arc<GlobalConfig>,
}

impl RequestEnricherLayer {
    pub fn new(sites: Vec<Arc<SiteConfig>>, global: Arc<GlobalConfig>) -> Self {
        Self { sites, global }
    }
}

impl<S> Layer<S> for RequestEnricherLayer {
    type Service = RequestEnricherService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestEnricherService::new(inner, self.sites.clone(), self.global.clone())
    }
}

/// Service that enriches requests with HttpWardContext
#[derive(Clone, Debug)]
pub struct RequestEnricherService<S> {
    inner: S,
    global: Arc<GlobalConfig>,
    // Cached WildMatch patterns for better performance
    cached_patterns: Vec<(Arc<SiteConfig>, Vec<WildMatch>)>,
}

impl<S> RequestEnricherService<S> {
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

impl<S, State> Service<State, Request<Body>> for RequestEnricherService<S>
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
        let request_content_type = extract_content_type_from_request(&request);

        // Find site by SNI from TLS context if available
        let site = self.find_site_by_sni(&ctx);
        let site_domain = site.as_ref().map(|s| s.domain.as_str());

        // Create and insert HttpWardContext into the context
        let enriched_context = HttpWardContext {
            client_addr,
            score: 0,
            request_content_type,
            response_content_type: ContentType::Unknown, // Will be set by ResponseEnricher
            site: site.clone(),
            global: self.global.clone(),
        };

        ctx.insert(enriched_context);

        trace!("enriched request with HttpWardContext: client_addr={}, request_content_type={:?}, site={:?}", 
               client_addr, request_content_type, site_domain);

        // Continue with the inner service
        self.inner.serve(ctx, request).await
    }
}
