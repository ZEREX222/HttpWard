// httpward-modules/httpward_log_module/src/lib.rs
// Refactored to use generic module export system

// Import our custom middleware
mod httpward_log_layer;
pub use httpward_log_layer::HttpWardLogLayer;

// Use the generic export macro with explicit module name
httpward_core::export_middleware_module!("httpward_log_module", HttpWardLogLayer);
