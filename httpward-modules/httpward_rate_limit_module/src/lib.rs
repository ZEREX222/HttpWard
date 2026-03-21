// httpward-modules/httpward_rate_limit_module/src/lib.rs
// HttpWard rate limit module

// Import our custom middleware
mod httpward_rate_limit_layer;
pub use httpward_rate_limit_layer::HttpWardRateLimitLayer;

// Import core modules
mod core;
pub use core::*;

// Use the generic export macro with automatic module name detection
// Will use "httpward_rate_limit_module" from Cargo.toml
httpward_core::export_middleware_module!(HttpWardRateLimitLayer);
