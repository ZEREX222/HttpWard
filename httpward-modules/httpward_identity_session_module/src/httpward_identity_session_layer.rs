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
use httpward_core::{module_log_debug, module_log_error};
use httpward_core::module_logging::ModuleLogger;
use rama::{http::{Request, Response, Body}, Context};
use async_trait::async_trait;

use crate::core::HttpWardIdentitySessionConfig;

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
        _ctx: Context<()>,
        _req: Request<Body>,
        next: Next<'_>,
    ) -> Result<Response<Body>, BoxError> {
        let _config = if let Some(httpward_ctx) = _ctx.get::<HttpWardContext>() {
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

        // TODO: Implement identity and session logic using config
        // For now, just pass through to next middleware
        next.run(_ctx, _req).await
    }

    fn name(&self) -> Option<&'static str> {
        Some("HttpWardIdentitySessionLayer")
    }
}


