use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use crate::core::server_models::SiteManager;
use crate::core::server_models::server_instance::ServerInstance;
use rama::http::headers::ContentType;
use rama::http::HeaderMap;
use serde::de::DeserializeOwned;
use crate::core::context::ExtensionsMap;



#[derive(Debug, Clone)]
pub struct HttpWardContext {
    pub client_ip: IpAddr,
    pub current_site: Option<Arc<SiteManager>>,
    pub server_instance: Arc<ServerInstance>,
    pub request_content_type: ContentType,
    pub response_content_type: ContentType,
    pub header_fp: Option<String>,
    pub ja4_fp: Option<String>,
    /// Request headers that can be modified by middleware during pipe processing
    pub request_headers: HeaderMap,
    /// Extensions map for storing arbitrary data during request lifetime.
    /// Allows middleware to share serialized objects without modifying context structure.
    pub extensions: ExtensionsMap,
    /// Cached matched route to avoid duplicate route resolution per request
    /// Set by DynamicModuleLoaderLayer, reused by RouteLayer
    pub matched_route: Option<crate::core::server_models::MatchedRoute>,
}

impl HttpWardContext {
    pub fn new(client_addr: SocketAddr, server_instance: Arc<ServerInstance>) -> Self {
        let client_ip = client_addr.ip();
        Self {
            client_ip,
            request_content_type: ContentType::text(),
            response_content_type: ContentType::text(),
            current_site: None,
            server_instance,
            header_fp: None,
            ja4_fp: None,
            request_headers: HeaderMap::new(),
            extensions: ExtensionsMap::new(),
            matched_route: None,
        }
    }
    
    /// Set the current site manager
    pub fn set_current_site(&mut self, site_manager: Arc<SiteManager>) {
        self.current_site = Some(site_manager);
    }
    
    /// Get route for the given path using current site manager
    pub fn get_route(&self, path: &str) -> Result<Option<crate::core::server_models::MatchedRoute>, crate::core::server_models::SiteManagerError> {
        match &self.current_site {
            Some(site_manager) => {
                match site_manager.get_route(path) {
                    Ok(route) => Ok(Some(route)),
                    Err(crate::core::server_models::SiteManagerError::NoMatch) => Ok(None),
                    Err(e) => Err(e),
                }
            }
            None => Ok(None),
        }
    }
    
    /// Check if context has a current site
    pub fn has_current_site(&self) -> bool {
        self.current_site.is_some()
    }
    
    /// Get site name if available
    pub fn site_name(&self) -> Option<&str> {
        self.current_site.as_ref().map(|sm| sm.site_name())
    }

    /// Get typed middleware config from cached matched route in this request context.
    pub fn middleware_config_typed_from_matched_route<T>(
        &self,
        middleware_name: &str,
    ) -> anyhow::Result<Option<Arc<T>>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let matched = match &self.matched_route {
            Some(matched) => matched,
            None => return Ok(None),
        };

        let route_with_strategy = crate::core::server_models::site_manager::RouteWithStrategy::new(
            matched.route.clone(),
            matched.active_strategy.clone(),
        );

        route_with_strategy.middleware_config_typed::<T>(middleware_name)
    }
}
