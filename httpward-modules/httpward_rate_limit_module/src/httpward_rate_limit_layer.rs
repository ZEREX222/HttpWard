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

use async_trait::async_trait;
use httpward_core::core::HttpWardContext;
use httpward_core::core::server_models::server_instance::ServerInstance;
use httpward_core::httpward_middleware::context::HttpwardMiddlewareContext;
use httpward_core::httpward_middleware::next::Next;
use httpward_core::httpward_middleware::{BoxError, HttpWardMiddleware};
use httpward_core::module_logging::ModuleLogger;
use httpward_core::{module_log_debug, module_log_error, module_log_info, module_log_warn};
use rama::http::{Body, HeaderMap, Request, Response};
use rama::net::fingerprint::Ja4;
use rama::net::tls::ProtocolVersion;
use std::sync::Arc;

use crate::core::{
    HttpWardRateLimitConfig, HttpWardRateLimitContext, RateLimitKeyKind, RateLimitScope,
    RouteScopeKey, SERVICE_KEY, init_global_manager,
};

/// Extract header fingerprint from specific headers
fn extract_header_fingerprint(headers: &HeaderMap) -> Option<String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let header_names = [
        "user-agent",
        "accept",
        "accept-language",
        "accept-encoding",
        "sec-ch-ua",
        "sec-ch-ua-platform",
        "sec-ch-ua-mobile",
    ];

    let mut hasher = DefaultHasher::new();
    let mut has_any_header = false;

    // Iterate in fixed order to keep deterministic fingerprint without HashMap/sort.
    for header_name in &header_names {
        if let Some(header_value) = headers.get(*header_name)
            && let Ok(value_str) = header_value.to_str()
        {
            has_any_header = true;
            hasher.write(header_name.as_bytes());
            hasher.write_u8(b':');
            for byte in value_str.as_bytes() {
                hasher.write_u8(byte.to_ascii_lowercase());
            }
            hasher.write_u8(b'|');
        }
    }

    if !has_any_header {
        return None;
    }

    Some(format!("{:x}", hasher.finish()))
}

pub struct HttpWardRateLimitLayer {}

impl HttpWardRateLimitLayer {
    pub fn new() -> Self {
        Self {}
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

#[async_trait]
impl HttpWardMiddleware for HttpWardRateLimitLayer {
    fn init(&self, server_instance: &Arc<ServerInstance>) -> Result<(), BoxError> {
        let manager = init_global_manager(); // Arc<RateLimitManager>

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
                    .init_from_config_sync(Some(route_key), &config)
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
        ctx: &mut HttpwardMiddlewareContext,
        req: Request<Body>,
        next: Next<'_>,
    ) -> Result<Response<Body>, BoxError> {
        let manager = init_global_manager(); // Arc<RateLimitManager>

        // ── Register manager in context so downstream middleware can access it ──
        // Must happen before any immutable borrow of ctx (e.g. get_httpward_context).
        ctx.set_service(SERVICE_KEY, manager.clone());
        // ─────────────────────────────────────────────────────────────────────────

        let httpward_ctx = ctx.get_httpward_context();

        let route_key = httpward_ctx.and_then(matched_route_key);
        let route_scope = httpward_ctx.and_then(matched_route_label);

        let mut ja4_fp = None;
        let client_ip = httpward_ctx
            .map(|context| context.client_ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        if let Some(st) = ctx.get_secure_transport()
            && let Some(client_hello) = st.client_hello()
        {
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

        let header_fp = extract_header_fingerprint(req.headers());

        let mut rate_limit_context = HttpWardRateLimitContext::new();

        if let Some(header_fp) = header_fp.clone() {
            rate_limit_context = rate_limit_context.with_header_fp(header_fp);
        }

        if let Some(ja4_fp) = ja4_fp.clone() {
            rate_limit_context = rate_limit_context.with_ja4_fp(ja4_fp);
        }

        module_log_info!(
            "HttpWardRateLimitLayer: Starting rate-limit check for scope {:?}",
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
            checks.push((RateLimitKeyKind::Ja4, RateLimitScope::Global, ja4.clone()));
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

        if !checks.is_empty() {
            match manager.check_all_with_results(&checks).await {
                Ok(results) => {
                    module_log_debug!(
                        "HttpWardRateLimitLayer: check_all_with_results — allowed={}, checks={}",
                        results.allowed,
                        results.checks.len(),
                    );
                    rate_limit_context = rate_limit_context.with_check_results(results);
                }
                Err(error) => {
                    module_log_error!(
                        "HttpWardRateLimitLayer: Error checking rate limits: {}",
                        error
                    );
                }
            }
        } else {
            module_log_debug!(
                "HttpWardRateLimitLayer: No active rules, skipping rate-limit checks"
            );
        }

        // Cross-DLL storage: available to subsequent dynamic middlewares.
        // Inserted after checks so results are included.
        if let Err(e) = ctx.insert_shared("httpward_rate_limit.context", &rate_limit_context) {
            module_log_warn!("Failed to write shared rate-limit context: {}", e);
        }

        // Best effort for static middleware that still reads typed Rama extensions.
        if let Some(rama_ctx) = ctx.rama_context_mut() {
            rama_ctx.insert(rate_limit_context);
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
