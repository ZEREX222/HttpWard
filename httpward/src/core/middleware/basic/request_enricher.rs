use rama::{
    http::{Request, HeaderMap},
    layer::Layer,
    service::Service,
    Context,
};
use std::fmt::Debug;
use std::sync::Arc;
use tracing::{debug, info, trace, warn};
use wildmatch::WildMatch;
use std::collections::HashMap;



use rama::http::Body;
use rama::net::fingerprint::Ja4;
use rama::net::tls::{ProtocolVersion, SecureTransport};
use rama::http::headers::ContentType;
use std::str::FromStr;
use httpward_core::config::{SiteConfig};
use httpward_core::core::HttpWardContext;
use httpward_core::core::server_models::server_instance::ServerInstance;

/// Extract content type from request headers
fn extract_content_type_from_request(request: &Request<Body>) -> ContentType {
    // Try to extract headers from the request
    if let Some(headers) = request.headers().get("content-type") {
        if let Ok(content_type_str) = headers.to_str() {
            return ContentType::from_str(content_type_str).unwrap_or_else(|_| ContentType::text());
        }
    }
    ContentType::text()
}

/// Extract header fingerprint from specific headers
fn extract_header_fingerprint(headers: &HeaderMap) -> Option<String> {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    
    let header_names = [
        "user-agent",
        "accept",
        "accept-language", 
        "accept-encoding",
        "sec-ch-ua",
        "sec-ch-ua-platform",
        "sec-ch-ua-mobile"
    ];
    
    let mut header_values = HashMap::new();
    
    for header_name in &header_names {
        if let Some(header_value) = headers.get(*header_name) {
            if let Ok(value_str) = header_value.to_str() {
                header_values.insert(*header_name, value_str.to_lowercase());
            }
        }
    }
    
    if header_values.is_empty() {
        return None;
    }
    
    // Create a deterministic string from header values
    let mut sorted_headers: Vec<_> = header_values.iter().collect();
    sorted_headers.sort_by_key(|(k, _)| *k);
    
    let combined_string = sorted_headers
        .iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect::<Vec<_>>()
        .join("|");
    
    // Create hash
    let mut hasher = DefaultHasher::new();
    combined_string.hash(&mut hasher);
    
    Some(format!("{:x}", hasher.finish()))
}

/// Layer that enriches request context with HttpWardContext containing client_addr, site and server_instance
#[derive(Clone, Debug)]
pub struct RequestEnricherLayer {
    sites: Vec<Arc<SiteConfig>>,
    server_instance: Arc<ServerInstance>,
}

impl RequestEnricherLayer {
    pub fn new(sites: Vec<Arc<SiteConfig>>, server_instance: Arc<ServerInstance>) -> Self {
        Self { sites, server_instance }
    }
}

impl<S> Layer<S> for RequestEnricherLayer {
    type Service = RequestEnricherService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestEnricherService::new(inner, self.sites.clone(), self.server_instance.clone())
    }
}

/// Service that enriches requests with HttpWardContext
#[derive(Clone, Debug)]
pub struct RequestEnricherService<S> {
    inner: S,
    server_instance: Arc<ServerInstance>,
    // Cached WildMatch patterns for better performance
    cached_patterns: Vec<(Arc<SiteConfig>, Vec<WildMatch>)>,
}

impl<S> RequestEnricherService<S> {
    pub fn new(inner: S, sites: Vec<Arc<SiteConfig>>, server_instance: Arc<ServerInstance>) -> Self {
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
            server_instance,
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
        // Extract client IP from context
        let client_ip = ctx.get::<rama::net::address::SocketAddress>()
            .map(|addr| *addr.ip_addr())
            .or_else(|| ctx.get::<std::net::SocketAddr>().map(|addr| addr.ip()))
            .unwrap_or_else(|| std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));

        let mut ja4_fp = None;
        let mut header_fp = None;

        // Try to get SecureTransport from context
        if let Some(st) = ctx.get::<SecureTransport>() {

            // ClientHello is available only if with_store_client_hello(true) was enabled
            if let Some(client_hello) = st.client_hello() {

                let pv = client_hello.protocol_version();

                let effective_version = match pv {
                    ProtocolVersion::Unknown(_) => ProtocolVersion::TLSv1_2,
                    other => other,
                };

                // Try to compute JA4 fingerprint
                match Ja4::compute_from_client_hello(client_hello, Some(effective_version)) {
                    Ok(ja4) => {
                        ja4_fp = Some(ja4.to_string());
                        debug!("JA4 fingerprint: {}", ja4_fp.as_ref().unwrap());
                    }
                    Err(e) => {
                        warn!("Failed to compute JA4 fingerprint: {}", e);
                    }
                }

            }
        }

        // Extract header fingerprint from specific headers
        header_fp = extract_header_fingerprint(request.headers());

        // Extract content type from request headers
        let request_content_type = extract_content_type_from_request(&request);

        // Find site by SNI from TLS context if available
        let site = self.find_site_by_sni(&ctx);
        let site_domain = site.as_ref().map(|s| s.domain.as_str());

        // Create and insert HttpWardContext into the context
        let enriched_context = HttpWardContext {
            client_ip,
            score: 0,
            request_content_type: request_content_type.clone(),
            response_content_type: ContentType::text(), // Will be set by ResponseEnricher
            site: site.clone(),
            server_instance: self.server_instance.clone(),
            ja4_fp,
            header_fp
        };

        ctx.insert(enriched_context);

        trace!("enriched request with HttpWardContext: client_ip={}, request_content_type={:?}, site={:?}", 
               client_ip, request_content_type, site_domain);

        // Continue with the inner service
        self.inner.serve(ctx, request).await
    }
}
