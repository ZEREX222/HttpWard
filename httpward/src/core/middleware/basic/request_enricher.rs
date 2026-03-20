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
use httpward_core::core::server_models::site_manager::SiteManager;
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
    server_instance: Arc<ServerInstance>,
}

impl RequestEnricherLayer {
    pub fn new(server_instance: Arc<ServerInstance>) -> Self {
        Self { server_instance }
    }
}

impl<S> Layer<S> for RequestEnricherLayer {
    type Service = RequestEnricherService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestEnricherService::new(inner, self.server_instance.clone())
    }
}

/// Service that enriches requests with HttpWardContext
#[derive(Clone, Debug)]
pub struct RequestEnricherService<S> {
    inner: S,
    server_instance: Arc<ServerInstance>,
    // Cached WildMatch patterns for better performance
    cached_patterns: Vec<(Arc<SiteManager>, Vec<WildMatch>)>,
}

impl<S> RequestEnricherService<S> {
    pub fn new(inner: S, server_instance: Arc<ServerInstance>) -> Self {
        // Pre-cache WildMatch patterns for all domains
        let cached_patterns = server_instance.site_managers.iter().map(|site_manager| {
            let all_domains = site_manager.site_config().get_all_domains();
            let patterns: Vec<WildMatch> = all_domains
                .iter()
                .map(|domain| WildMatch::new(domain))
                .collect();
            (site_manager.clone(), patterns)
        }).collect();

        Self { 
            inner, 
            server_instance,
            cached_patterns,
        }
    }

    /// Find site manager by domain from either Host header (HTTP) or SNI (HTTPS)
    fn find_site_by_domain<State>(&self, ctx: &Context<State>, request: &Request<Body>) -> Option<Arc<SiteManager>> {
        // First, check if there's a site with no domain restrictions
        let unrestricted_site = self.server_instance.site_managers.iter().find(|site_manager| {
            !site_manager.site_config().has_domains()
        });
        
        let mut domain_to_match = None;
        
        // Try to get domain from Host header (HTTP) first
        if let Some(host_header) = request.headers().get("host") {
            if let Ok(host_str) = host_header.to_str() {
                // Remove port if present (e.g., "example.com:8080" -> "example.com")
                domain_to_match = Some(host_str.split(':').next().unwrap_or(host_str).to_lowercase());
            }
        }
        
        // If no Host header, try SNI from TLS context (HTTPS)
        if domain_to_match.is_none() {
            if let Some(st) = ctx.get::<rama::net::tls::SecureTransport>() {
                if let Some(client_hello) = st.client_hello() {
                    if let Some(sni) = client_hello.ext_server_name() {
                        domain_to_match = Some(sni.to_string().to_lowercase());
                    }
                }
            }
        }
        
        // If we have a domain to match, find the corresponding site
        if let Some(domain) = domain_to_match {
            // Find site manager using cached WildMatch patterns (more efficient)
            if let Some((site_manager, _)) = self.cached_patterns.iter().find(|(_, patterns)| {
                patterns.iter().any(|pattern| pattern.matches(&domain))
            }) {
                return Some(site_manager.clone());
            }
        }
        
        // If no domain match found, return unrestricted site if available
        unrestricted_site.cloned()
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

        // Find site by domain from either Host header (HTTP) or SNI (HTTPS)
        let site = self.find_site_by_domain(&ctx, &request);
        let site_domain = site.as_ref().map(|sm| sm.site_name());

        // Create and insert HttpWardContext into the context
        let mut enriched_context = HttpWardContext {
            client_ip,
            request_content_type: request_content_type.clone(),
            response_content_type: ContentType::text(), // Will be set by ResponseEnricher
            current_site: site.clone(),
            server_instance: self.server_instance.clone(),
            ja4_fp,
            header_fp,
            request_headers: request.headers().clone(),
            extensions: httpward_core::core::context::ExtensionsMap::new(),
            matched_route: None, // Will be set by DynamicModuleLoaderLayer
        };

        ctx.insert(enriched_context);

        trace!("enriched request with HttpWardContext: client_ip={}, request_content_type={:?}, site={:?}", 
               client_ip, request_content_type, site_domain);

        // Continue with the inner service
        self.inner.serve(ctx, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rama::Context;
    use httpward_core::config::{SiteConfig, GlobalConfig};
    use httpward_core::core::server_models::{ServerInstance, SiteManager, listener::ListenerKey};
    use std::sync::Arc;
    
    #[tokio::test]
    async fn test_find_site_by_domain_with_unrestricted_site() {
        // Create site config with no domains (unrestricted)
        let mut site_config = SiteConfig::default();
        site_config.domain = "".to_string(); // Empty domain means unrestricted
        site_config.domains = vec![]; // No additional domains
        
        let site_config_arc = Arc::new(site_config);
        let site_manager = SiteManager::new(site_config_arc.clone(), None).unwrap();
        let site_manager_arc = Arc::new(site_manager);
        
        // Create server instance with unrestricted site
        let server_instance = ServerInstance {
            bind: ListenerKey { host: "127.0.0.1".to_string(), port: 8080 },
            site_managers: vec![site_manager_arc.clone()],
            global: GlobalConfig::default(),
        };
        let server_instance_arc = Arc::new(server_instance);
        
        // Create request enricher service
        let service = RequestEnricherService::new((), server_instance_arc.clone());
        
        // Test with empty context (no SNI) and empty request
        let ctx = Context::default();
        let request = Request::builder().body(Body::empty()).unwrap();
        let found_site = service.find_site_by_domain(&ctx, &request);
        
        // Should return the unrestricted site
        assert!(found_site.is_some());
        assert_eq!(found_site.unwrap().site_name(), site_config_arc.domain);
    }
    
    #[tokio::test]
    async fn test_find_site_by_domain_prefer_domain_match_over_unrestricted() {
        // Create unrestricted site
        let mut unrestricted_config = SiteConfig::default();
        unrestricted_config.domain = "".to_string();
        let unrestricted_site = SiteManager::new(Arc::new(unrestricted_config), None).unwrap();
        
        // Create domain-specific site
        let mut domain_config = SiteConfig::default();
        domain_config.domain = "example.com".to_string();
        let domain_site = SiteManager::new(Arc::new(domain_config), None).unwrap();
        
        // Create server instance with both sites
        let server_instance = ServerInstance {
            bind: ListenerKey { host: "127.0.0.1".to_string(), port: 8080 },
            site_managers: vec![Arc::new(unrestricted_site), Arc::new(domain_site)],
            global: GlobalConfig::default(),
        };
        let server_instance_arc = Arc::new(server_instance);
        
        // Create request enricher service
        let service = RequestEnricherService::new((), server_instance_arc.clone());
        
        // Test with context and request that has no Host header and no SNI
        let ctx = Context::default();
        let request = Request::builder().body(Body::empty()).unwrap();
        let found_site = service.find_site_by_domain(&ctx, &request);
        
        // Should return the unrestricted site since no SNI match found
        assert!(found_site.is_some());
        assert_eq!(found_site.unwrap().site_name(), "");
    }
    
    #[tokio::test]
    async fn test_find_site_by_domain_with_host_header() {
        // Create domain-specific site
        let mut domain_config = SiteConfig::default();
        domain_config.domain = "example.com".to_string();
        let domain_site = SiteManager::new(Arc::new(domain_config), None).unwrap();
        
        // Create server instance with domain site
        let server_instance = ServerInstance {
            bind: ListenerKey { host: "127.0.0.1".to_string(), port: 8080 },
            site_managers: vec![Arc::new(domain_site)],
            global: GlobalConfig::default(),
        };
        let server_instance_arc = Arc::new(server_instance);
        
        // Create request enricher service
        let service = RequestEnricherService::new((), server_instance_arc.clone());
        
        // Test with request that has Host header
        let ctx = Context::default();
        let request = Request::builder()
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();
        let found_site = service.find_site_by_domain(&ctx, &request);
        
        // Should return the domain-specific site
        assert!(found_site.is_some());
        assert_eq!(found_site.unwrap().site_name(), "example.com");
    }
    
    #[tokio::test]
    async fn test_find_site_by_domain_with_port_in_host_header() {
        // Create domain-specific site
        let mut domain_config = SiteConfig::default();
        domain_config.domain = "example.com".to_string();
        let domain_site = SiteManager::new(Arc::new(domain_config), None).unwrap();
        
        // Create server instance with domain site
        let server_instance = ServerInstance {
            bind: ListenerKey { host: "127.0.0.1".to_string(), port: 8080 },
            site_managers: vec![Arc::new(domain_site)],
            global: GlobalConfig::default(),
        };
        let server_instance_arc = Arc::new(server_instance);
        
        // Create request enricher service
        let service = RequestEnricherService::new((), server_instance_arc.clone());
        
        // Test with request that has Host header with port
        let ctx = Context::default();
        let request = Request::builder()
            .header("host", "example.com:8080")
            .body(Body::empty())
            .unwrap();
        let found_site = service.find_site_by_domain(&ctx, &request);
        
        // Should return the domain-specific site (port should be stripped)
        assert!(found_site.is_some());
        assert_eq!(found_site.unwrap().site_name(), "example.com");
    }
    
    #[tokio::test]
    async fn test_find_site_by_domain_host_header_takes_precedence() {
        // Create two different domain sites
        let mut config1 = SiteConfig::default();
        config1.domain = "example.com".to_string();
        let site1 = SiteManager::new(Arc::new(config1), None).unwrap();
        
        let mut config2 = SiteConfig::default();
        config2.domain = "other.com".to_string();
        let site2 = SiteManager::new(Arc::new(config2), None).unwrap();
        
        // Create server instance with both sites
        let server_instance = ServerInstance {
            bind: ListenerKey { host: "127.0.0.1".to_string(), port: 8080 },
            site_managers: vec![Arc::new(site1), Arc::new(site2)],
            global: GlobalConfig::default(),
        };
        let server_instance_arc = Arc::new(server_instance);
        
        // Create request enricher service
        let service = RequestEnricherService::new((), server_instance_arc.clone());
        
        // Test with request that has Host header (should match example.com)
        let ctx = Context::default();
        let request = Request::builder()
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();
        let found_site = service.find_site_by_domain(&ctx, &request);
        
        // Should return example.com site
        assert!(found_site.is_some());
        assert_eq!(found_site.unwrap().site_name(), "example.com");
    }
}
