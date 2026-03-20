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
        ctx: Context<()>,
        req: Request<Body>,
        next: Next<'_>,
    ) -> Result<Response<Body>, BoxError> {
        // Load configuration using the macro
        let _config = match httpward_core::get_module_config_from_current_crate!(HttpWardIdentitySessionConfig, &ctx, &req) {
            Ok(config) => {
                module_log_debug!("HttpWardIdentitySessionLayer config loaded successfully: {:?}", config);
                config
            }
            Err(e) => {
                module_log_error!("Failed to load HttpWardIdentitySessionLayer configuration: {}, using defaults", e);
                HttpWardIdentitySessionConfig::default()
            }
        };

        // TODO: Implement identity and session logic using config
        // For now, just pass through to next middleware
        next.run(ctx, req).await
    }

    fn name(&self) -> Option<&'static str> {
        Some("HttpWardIdentitySessionLayer")
    }
}


