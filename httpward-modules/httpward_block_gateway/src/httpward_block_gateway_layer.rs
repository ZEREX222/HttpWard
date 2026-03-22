// httpward-modules/httpward_block_gateway/src/httpward_block_gateway_layer.rs
//
// HttpWard Block Gateway Layer
//
// Reads the `HttpWardRateLimitContext` produced by `HttpWardRateLimitLayer`
// and, when the request has been rate-limited, returns an error response.
// Every individual check result is logged so operators can see exactly which
// rule triggered the block.
//
// Pipeline order:
//   1. HttpWardRateLimitLayer  — performs checks, stores results in context
//   2. HttpWardBlockGatewayLayer — reads results, logs them, blocks if needed
//   3. ... remaining middleware / upstream

use async_trait::async_trait;
use httpward_core::error::ErrorHandler;
use httpward_core::httpward_middleware::context::HttpwardMiddlewareContext;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::next::Next;
use httpward_core::httpward_middleware::types::BoxError;
use httpward_core::module_logging::ModuleLogger;
use httpward_core::{module_log_debug, module_log_error, module_log_info, module_log_warn};
use httpward_rate_limit_module::RateLimitCheckStatus;
use rama::http::{Body, Request, Response, StatusCode};

/// Block gateway middleware.
///
/// Must be placed **after** `HttpWardRateLimitLayer` in the middleware
/// pipeline so that rate-limit check results are already in context.
#[derive(Clone, Debug, Default)]
pub struct HttpWardBlockGatewayLayer;

impl HttpWardBlockGatewayLayer {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl HttpWardMiddleware for HttpWardBlockGatewayLayer {
    async fn handle(
        &self,
        ctx: &mut HttpwardMiddlewareContext,
        req: Request<Body>,
        next: Next<'_>,
    ) -> Result<Response<Body>, BoxError> {
        module_log_debug!("HttpWardBlockGatewayLayer: handle called");

        // ── Retrieve rate-limit context produced by HttpWardRateLimitLayer ────
        let rl_ctx = httpward_rate_limit_module::get_rate_limit_context_from_context(ctx);

        let Some(rl_ctx) = rl_ctx else {
            module_log_debug!(
                "HttpWardBlockGatewayLayer: no rate-limit context found, skipping block check"
            );
            return next.run(ctx, req).await;
        };

        // ── Log every individual check result ─────────────────────────────────
        if let Some(results) = &rl_ctx.check_results {
            for check in &results.checks {
                match check.status {
                    RateLimitCheckStatus::Allow => {
                        module_log_debug!(
                            "HttpWardBlockGatewayLayer: [{}] {:?} scope={} value={} → ALLOW",
                            env!("CARGO_PKG_NAME"),
                            check.kind,
                            check.scope_type,
                            check.value,
                        );
                    }
                    RateLimitCheckStatus::New => {
                        module_log_debug!(
                            "HttpWardBlockGatewayLayer: [{}] {:?} scope={} value={} → NEW (first seen)",
                            env!("CARGO_PKG_NAME"),
                            check.kind,
                            check.scope_type,
                            check.value,
                        );
                    }
                    RateLimitCheckStatus::Declined => {
                        module_log_warn!(
                            "HttpWardBlockGatewayLayer: [{}] {:?} scope={} value={} → DECLINED (rate limit exceeded)",
                            env!("CARGO_PKG_NAME"),
                            check.kind,
                            check.scope_type,
                            check.value,
                        );
                    }
                }
            }

            module_log_info!(
                "HttpWardBlockGatewayLayer: overall result — allowed={}, total_checks={}",
                results.allowed,
                results.checks.len(),
            );
        } else {
            module_log_debug!(
                "HttpWardBlockGatewayLayer: rate-limit context present but no check results (no rules configured)"
            );
        }

        // ── Block if rate-limited ──────────────────────────────────────────────
        if rl_ctx.is_rate_limited() {
            let status = StatusCode::TOO_MANY_REQUESTS;
            module_log_warn!(
                "HttpWardBlockGatewayLayer: request blocked with status {}",
                status.as_u16()
            );

            let error_handler = ErrorHandler::default();
            match error_handler.create_error_response_with_code(status) {
                Ok(response) => return Ok(response),
                Err(e) => {
                    module_log_error!(
                        "HttpWardBlockGatewayLayer: failed to create error response: {}",
                        e
                    );
                    return Err(format!("Failed to create error response: {}", e).into());
                }
            }
        }

        module_log_debug!(
            "HttpWardBlockGatewayLayer: request allowed, forwarding to next middleware"
        );
        next.run(ctx, req).await
    }

    fn optional_dependencies(&self) -> Vec<&'static str> {
        vec!["httpward_rate_limit_module"]
    }
    
    fn name(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_NAME"))
    }
}
