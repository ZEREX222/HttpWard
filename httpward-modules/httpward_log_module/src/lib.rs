// httpward-modules/httpward_log_module/src/lib.rs
// Refactored to use generic module export system

// Import our custom middleware
mod httpward_log_layer;
use httpward_log_layer::HttpWardLogLayer;

// Use the generic export macro with automatic module name detection
// Will use "httpward_log_module" from Cargo.toml
httpward_core::export_middleware_module!(HttpWardLogLayer);
