// httpward-modules/httpward_identity_session_module/src/lib.rs
// HttpWard identity and session module

// Import our custom middleware
mod httpward_identity_session_layer;
pub use httpward_identity_session_layer::HttpWardIdentitySessionLayer;

// Import core modules
mod core;
pub use core::*;

// Use the generic export macro with automatic module name detection
// Will use "httpward_identity_session_module" from Cargo.toml
httpward_core::export_middleware_module!(HttpWardIdentitySessionLayer);
