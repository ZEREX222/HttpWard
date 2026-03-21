// httpward-modules/httpward_identity_session_module/src/httpward_identity_session_layer.rs
// HttpWard Identity and Session Layer
// 
// This file will contain the implementation of HttpWardIdentitySessionLayer
// which will provide identity and session management capabilities for HttpWard.
// 
// Future implementation will include:
// - User authentication
// - Session management
// - Token handling
// - Identity verification
// - Session persistence

// TODO: Implement HttpWardIdentitySessionLayer
// This will be a middleware that handles user identity and session management

use httpward_core::httpward_middleware::{HttpWardMiddleware, BoxError};
use httpward_core::httpward_middleware::next::Next;
use httpward_core::core::HttpWardContext;
use httpward_core::{module_log_debug, module_log_error, module_log_info, module_log_warn};
use httpward_core::module_logging::ModuleLogger;
use rama::{http::{Request, Response, Body, HeaderMap}, Context};
use rama::net::fingerprint::Ja4;
use rama::net::tls::{ProtocolVersion, SecureTransport};
use async_trait::async_trait;
use std::collections::HashMap;

use crate::core::HttpWardIdentitySessionConfig;
use crate::core::HttpWardIdentitySessionContext;

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

pub struct HttpWardIdentitySessionLayer {
}

impl HttpWardIdentitySessionLayer {
    pub fn new() -> Self {
        Self {
        }
    }
}

impl Default for HttpWardIdentitySessionLayer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HttpWardMiddleware for HttpWardIdentitySessionLayer {
    async fn handle(
        &self,
        mut ctx: Context<()>,
        req: Request<Body>,
        next: Next<'_>,
    ) -> Result<Response<Body>, BoxError> {
        let _config = if let Some(httpward_ctx) = ctx.get::<HttpWardContext>() {
            match httpward_ctx.middleware_config_typed_from_matched_route::<HttpWardIdentitySessionConfig>("HttpWardIdentitySessionLayer") {
                Ok(Some(config)) => {
                    module_log_debug!("HttpWardIdentitySessionLayer config loaded from HttpWardContext.matched_route: {:?}", config);
                    config
                }
                Ok(None) => {
                    module_log_debug!("HttpWardIdentitySessionLayer config not found in HttpWardContext.matched_route, using defaults");
                    std::sync::Arc::new(HttpWardIdentitySessionConfig::default())
                }
                Err(e) => {
                    module_log_error!("Failed to parse HttpWardIdentitySessionLayer config from HttpWardContext.matched_route: {}, using defaults", e);
                    std::sync::Arc::new(HttpWardIdentitySessionConfig::default())
                }
            }
        } else {
            std::sync::Arc::new(HttpWardIdentitySessionConfig::default())
        };

        // Extract fingerprints
        let mut ja4_fp = None;
        let mut header_fp = None;

        // Try to get JA4 fingerprint from SecureTransport
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
                        module_log_info!("JA4 fingerprint: {}", ja4_fp.as_ref().unwrap());
                    }
                    Err(e) => {
                        module_log_warn!("Failed to compute JA4 fingerprint: {}", e);
                    }
                }
            }
        }

        // Extract header fingerprint from request headers
        header_fp = extract_header_fingerprint(req.headers());

        // Create identity session context with fingerprints
        let mut identity_context = HttpWardIdentitySessionContext::new();
        
        if let Some(header_fp) = header_fp {
            identity_context = identity_context.with_header_fp(header_fp);
        }
        
        if let Some(ja4_fp) = ja4_fp {
            identity_context = identity_context.with_ja4_fp(ja4_fp);
        }

        // Store the identity context directly in the context
        ctx.insert(identity_context);

        module_log_info!("HttpWardIdentitySessionLayer: Starting identity/session processing");

        // TODO: Implement identity and session logic using config
        // For now, just pass through to next middleware
        let result = next.run(ctx, req).await;
        
        module_log_info!("HttpWardIdentitySessionLayer: Finished identity/session processing");
        
        result
    }

    fn name(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_NAME"))
    }
}


