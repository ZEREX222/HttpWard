// httpward-modules/httpward_log_module/src/lib.rs
// Refactored to use generic module export system

// Import our custom middleware
mod httpward_log_layer;
pub use httpward_log_layer::HttpWardLogLayer;

// Name is taken automatically from CARGO_PKG_NAME ("httpward_log_module")
httpward_core::export_middleware_module!(HttpWardLogLayer);
