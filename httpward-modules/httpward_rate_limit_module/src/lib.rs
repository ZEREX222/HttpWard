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

// Use the generic export macro with explicit module name
httpward_core::export_middleware_module!("httpward_rate_limit_module", HttpWardRateLimitLayer);
