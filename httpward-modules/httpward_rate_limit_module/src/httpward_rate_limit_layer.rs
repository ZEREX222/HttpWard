// httpward-modules/httpward_rate_limit_module/src/httpward_rate_limit_layer.rs
// HttpWard Rate Limit Layer
//
// This file contains the implementation of HttpWardRateLimitLayer
// which provides rate limiting capabilities for HttpWard.
//
// Future implementation will include:
// - Rate limiting policy evaluation
// - Client quota tracking
// - Token bucket / leaky bucket handling
// - Fingerprint-aware limits
// - Storage-backed limiter state

// TODO: Implement HttpWardRateLimitLayer
// This will be a middleware that handles request rate limiting

use async_trait::async_trait;
use httpward_core::core::HttpWardContext;
use httpward_core::core::server_models::server_instance::ServerInstance;
use httpward_core::httpward_middleware::next::Next;
use httpward_core::httpward_middleware::{BoxError, HttpWardMiddleware};
use httpward_core::module_logging::ModuleLogger;
use httpward_core::{module_log_debug, module_log_error, module_log_info, module_log_warn};
use rama::net::fingerprint::Ja4;
use rama::net::tls::{ProtocolVersion, SecureTransport};
use rama::{http::{Body, HeaderMap, Request, Response, StatusCode}, Context};
use std::collections::HashMap;
use std::sync::Arc;

use crate::core::{
    init_global_manager, HttpWardRateLimitConfig, HttpWardRateLimitContext, RateLimitKeyKind,
    RateLimitScope, RouteScopeKey,
};

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

pub struct HttpWardRateLimitLayer {
}

impl HttpWardRateLimitLayer {
    pub fn new() -> Self {
        Self {
        }
    }
}

impl Default for HttpWardRateLimitLayer {
    fn default() -> Self {
        Self::new()
    }
}

fn matched_route_key(httpward_ctx: &HttpWardContext) -> Option<RouteScopeKey> {
    let matched_route = httpward_ctx.matched_route.as_ref()?;
    Some(RouteScopeKey::from_arc_ptr(&matched_route.route))
}

fn matched_route_label(httpward_ctx: &HttpWardContext) -> Option<String> {
    let matched_route = httpward_ctx.matched_route.as_ref()?;
    let matcher = matched_route.route.get_match();

    if let Some(path) = &matcher.path {
        return Some(format!("path:{path}"));
    }

    if let Some(path_regex) = &matcher.path_regex {
        return Some(format!("regex:{path_regex}"));
    }

    None
}

fn status_code_or_default(status_code: u16) -> StatusCode {
    StatusCode::from_u16(status_code).unwrap_or(StatusCode::TOO_MANY_REQUESTS)
}

/// Try to load rate limit config from matched route.
fn load_config_from_context(httpward_ctx: &HttpWardContext) -> std::sync::Arc<HttpWardRateLimitConfig> {
    match httpward_ctx.middleware_config_typed_from_matched_route::<HttpWardRateLimitConfig>(env!("CARGO_PKG_NAME")) {
        Ok(Some(config)) => {
            module_log_debug!("Loaded rate-limit config from matched route");
            config
        }
        Ok(None) => {
            module_log_debug!("No rate-limit config in matched route, using defaults");
            std::sync::Arc::new(HttpWardRateLimitConfig::default())
        }
        Err(e) => {
            module_log_warn!("Failed to parse rate-limit config: {}", e);
            std::sync::Arc::new(HttpWardRateLimitConfig::default())
        }
    }
}

#[async_trait]
impl HttpWardMiddleware for HttpWardRateLimitLayer {
    fn init(&self, server_instance: &Arc<ServerInstance>) -> Result<(), BoxError> {
        let manager = init_global_manager();

        for site_manager in &server_instance.site_managers {
            let site_name = site_manager.site_domains();

            for route_with_strategy in site_manager.routes_with_strategy() {
                let config = match route_with_strategy
                    .middleware_config_typed::<HttpWardRateLimitConfig>(env!("CARGO_PKG_NAME"))
                {
                    Ok(Some(config)) => config,
                    Ok(None) => continue,
                    Err(e) => {
                        module_log_warn!(
                            "Failed to parse rate-limit config for site '{}': {}. Using defaults for this route.",
                            site_name,
                            e
                        );
                        Arc::new(HttpWardRateLimitConfig::default())
                    }
                };

                let route_key = RouteScopeKey::from_arc_ptr(&route_with_strategy.route);
                if let Err(e) = manager
                    .init_from_config_sync(&site_name, Some(route_key), &config)
                    .map_err(|e| format!("Failed to init config: {}", e))
                {
                    module_log_error!(
                        "Failed to initialize rate limit config for site '{}': {}",
                        site_name,
                        e
                    );
                }
            }
        }

        Ok(())
    }

    async fn handle(
        &self,
        mut ctx: Context<()>,
        req: Request<Body>,
        next: Next<'_>,
    ) -> Result<Response<Body>, BoxError> {
        let httpward_ctx = ctx.get::<HttpWardContext>();

        let site_name = httpward_ctx
            .and_then(|c| c.site_domains())
            .unwrap_or_else(|| "default".to_string());

        let route_key = httpward_ctx.and_then(matched_route_key);
        let route_scope = httpward_ctx.and_then(matched_route_label);
        let manager = init_global_manager();

        let config = if let Some(httpward_ctx) = httpward_ctx {
            load_config_from_context(httpward_ctx)
        } else {
            module_log_warn!("HttpWardContext not found, rate limiter will use default site scope only");
            std::sync::Arc::new(HttpWardRateLimitConfig::default())
        };

        // Convert YAML config to internal format
        let internal_config = config.to_internal();

        let mut ja4_fp = None;
        let client_ip = httpward_ctx
            .map(|context| context.client_ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        if let Some(st) = ctx.get::<SecureTransport>() {
            if let Some(client_hello) = st.client_hello() {
                let pv = client_hello.protocol_version();
                let effective_version = match pv {
                    ProtocolVersion::Unknown(_) => ProtocolVersion::TLSv1_2,
                    other => other,
                };

                match Ja4::compute_from_client_hello(client_hello, Some(effective_version)) {
                    Ok(ja4) => {
                        ja4_fp = Some(ja4.to_string());
                        module_log_debug!("JA4 fingerprint: {}", ja4_fp.as_ref().unwrap());
                    }
                    Err(e) => {
                        module_log_debug!("Failed to compute JA4 fingerprint: {}", e);
                    }
                }
            }
        }

        let header_fp = extract_header_fingerprint(req.headers());

        let mut rate_limit_context = HttpWardRateLimitContext::new()
            .with_site_name(site_name.clone())
            .with_client_ip(client_ip.clone());

        if let Some(route_scope) = route_scope.clone() {
            rate_limit_context = rate_limit_context.with_matched_route_scope(route_scope);
        }

        if let Some(header_fp) = header_fp.clone() {
            rate_limit_context = rate_limit_context.with_header_fp(header_fp);
        }

        if let Some(ja4_fp) = ja4_fp.clone() {
            rate_limit_context = rate_limit_context.with_ja4_fp(ja4_fp);
        }

        ctx.insert(rate_limit_context);

        module_log_info!(
            "HttpWardRateLimitLayer: Starting rate-limit check for site '{}' and scope {:?}",
            site_name,
            route_scope
        );

        let mut checks = Vec::new();

        checks.push((
            RateLimitKeyKind::Ip,
            RateLimitScope::Global,
            client_ip.clone(),
        ));

        if let Some(route_key) = route_key {
            checks.push((
                RateLimitKeyKind::Ip,
                RateLimitScope::Route(route_key),
                client_ip.clone(),
            ));
        }

        if let Some(ja4) = ja4_fp.as_ref() {
            checks.push((
                RateLimitKeyKind::Ja4,
                RateLimitScope::Global,
                ja4.clone(),
            ));
            if let Some(route_key) = route_key {
                checks.push((
                    RateLimitKeyKind::Ja4,
                    RateLimitScope::Route(route_key),
                    ja4.clone(),
                ));
            }
        }

        if let Some(header_value) = header_fp.as_ref() {
            checks.push((
                RateLimitKeyKind::HeaderFingerprint,
                RateLimitScope::Global,
                header_value.clone(),
            ));
            if let Some(route_key) = route_key {
                checks.push((
                    RateLimitKeyKind::HeaderFingerprint,
                    RateLimitScope::Route(route_key),
                    header_value.clone(),
                ));
            }
        }

        if checks.is_empty() {
            module_log_debug!(
                "HttpWardRateLimitLayer: No active rules for site '{}', skipping rate-limit checks",
                site_name
            );
            return next.run(ctx, req).await;
        }

        match manager.check_all(&site_name, &checks).await {
            Ok(allowed) => {
                if !allowed {
                    module_log_warn!(
                        "HttpWardRateLimitLayer: Request rate limited for site '{}', scope {:?}, IP: {}",
                        site_name,
                        route_scope,
                        client_ip
                    );

                    return Ok(Response::builder()
                        .status(status_code_or_default(internal_config.response.status_code))
                        .body(Body::from(internal_config.response.body.clone()))
                        .unwrap());
                }
            }
            Err(error) => {
                module_log_error!(
                    "HttpWardRateLimitLayer: Error checking rate limits for site '{}': {}",
                    site_name,
                    error
                );
            }
        }

        module_log_debug!("HttpWardRateLimitLayer: Request allowed, proceeding to next middleware");

        let result = next.run(ctx, req).await;

        module_log_debug!("HttpWardRateLimitLayer: Finished rate-limit processing");

        result
    }

    fn name(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_NAME"))
    }
}




