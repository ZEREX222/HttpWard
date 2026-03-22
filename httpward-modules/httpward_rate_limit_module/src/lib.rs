// httpward-modules/httpward_rate_limit_module/src/lib.rs
// HttpWard rate limit module

// Import our custom middleware
mod httpward_rate_limit_layer;
pub use httpward_rate_limit_layer::HttpWardRateLimitLayer;

// Import core modules
mod core;
pub use core::*;

// Export InternalRateLimitRule for tests
pub use core::httpward_rate_limit_config::InternalRateLimitRule;

// Name is taken automatically from CARGO_PKG_NAME ("httpward_rate_limit_module")
httpward_core::export_middleware_module!(HttpWardRateLimitLayer);

// ─── Typed accessors ─────────────────────────────────────────────────────────
//
// The downcast MUST happen here — inside the rate-limit DLL binary — so that
// the `TypeId` for `RateLimitManager` matches the one used when the `Arc` was
// stored.  Calling `downcast` on the raw `Arc<dyn Any>` from a different DLL
// binary would always fail due to per-binary `TypeId` instability.

use httpward_core::httpward_middleware::context::HttpwardMiddlewareContext;
use std::sync::Arc;

/// Retrieve the `Arc<RateLimitManager>` that was registered in `ctx` by
/// `HttpWardRateLimitLayer::handle()`.
///
/// Returns `Some` only when `HttpWardRateLimitLayer` appears **before** the
/// calling middleware in the same pipeline (it registers the manager into the
/// context before invoking `next`).
///
/// # Example — inside another middleware
/// ```rust,ignore
/// use httpward_rate_limit_module::get_manager_from_context;
///
/// async fn handle(&self, ctx: &mut HttpwardMiddlewareContext, req, next) {
///     if let Some(manager) = get_manager_from_context(ctx) {
///         let stats = manager.stats().await?;
///     }
///     next.run(ctx, req).await
/// }
/// ```
pub fn get_manager_from_context(
    ctx: &HttpwardMiddlewareContext,
) -> Option<Arc<core::rate_limit_manager::RateLimitManager>> {
    ctx.get_service_raw(core::rate_limit_manager::SERVICE_KEY)
        .and_then(|arc| {
            arc.downcast::<core::rate_limit_manager::RateLimitManager>()
                .ok()
        })
}

/// Retrieve the [`HttpWardRateLimitContext`] stored by `HttpWardRateLimitLayer`
/// in shared middleware context storage.
///
/// Returns `Some` only when `HttpWardRateLimitLayer` has already run for this
/// request (i.e. it is positioned **before** the calling middleware in the
/// pipeline).
///
/// Internally this deserialises from the JSON blob stored under
/// `"httpward_rate_limit.context"` — safe to call across DLL boundaries.
///
/// # Example
/// ```rust,ignore
/// use httpward_rate_limit_module::get_rate_limit_context_from_context;
///
/// async fn handle(&self, ctx: &mut HttpwardMiddlewareContext, req, next) {
///     if let Some(rl_ctx) = get_rate_limit_context_from_context(ctx) {
///         if rl_ctx.is_rate_limited() {
///             // block or log
///         }
///     }
///     next.run(ctx, req).await
/// }
/// ```
pub fn get_rate_limit_context_from_context(
    ctx: &HttpwardMiddlewareContext,
) -> Option<core::httpward_rate_limit_context::HttpWardRateLimitContext> {
    ctx.get_shared_typed("httpward_rate_limit.context")
}
