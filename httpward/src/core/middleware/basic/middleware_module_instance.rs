// httpward/src/core/middleware/basic/module_loader
// Module loader using libloading and raw pointers.
// Comments/in-code text in English.

use std::sync::Arc;
use std::error::Error;
use std::os::raw::c_char;
use libloading::Library;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::pipe::MiddlewareFatPtr;
use httpward_core::module_logging::host_functions::*;
use tracing::warn;

/// C-ABI types exported by module
type CreateFn = unsafe extern "C" fn() -> MiddlewareFatPtr;
type DestroyFn = unsafe extern "C" fn(MiddlewareFatPtr);
type SetLoggerFn = unsafe extern "C" fn(
    extern "C" fn(*const c_char),  // error
    extern "C" fn(*const c_char),  // warn
    extern "C" fn(*const c_char),  // info
    extern "C" fn(*const c_char),  // debug
    extern "C" fn(*const c_char),  // trace
);


/// A middleware module instance.
/// Keeps the `Library` alive as long as module is used.
pub struct MiddlewareModuleInstance {
    lib: Option<Arc<Library>>,
    destroy: Option<DestroyFn>,
    ptr: Option<MiddlewareFatPtr>,
}

impl MiddlewareModuleInstance {
    /// Create middleware instance from Arc<Library>.
    /// Safety: host and module must agree on ABI.
    pub unsafe fn create_from_arc(lib: Arc<Library>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!(target: "module_loader", "Creating middleware instance from shared library");
        tracing::info!(target: "module_loader", "Getting function symbols from library");
        
        // Get symbols from the library reference
        let set_logger: libloading::Symbol<SetLoggerFn> = unsafe { (&*lib).get(b"module_set_logger")? };
        unsafe { set_logger(host_log_error, host_log_warn, host_log_info, host_log_debug, host_log_trace) };
        tracing::info!(target: "module_loader", "Module loggers set");
        
        // get symbols
        let create: libloading::Symbol<CreateFn> = unsafe { (&*lib).get(b"create_middleware")? };
        let destroy: libloading::Symbol<DestroyFn> = unsafe { (&*lib).get(b"destroy_middleware")? };
        tracing::info!(target: "module_loader", "Function symbols obtained, creating middleware instance");
        let ptr = unsafe { create() };
        tracing::info!(target: "module_loader", "Middleware instance created successfully");
        
        // Copy the function pointers before moving lib
        let destroy_fn = *destroy;
        
        Ok(Self { lib: Some(lib), destroy: Some(destroy_fn), ptr: Some(ptr) })
    }

    /// Manually destroy the module and free resources.
    /// This is called automatically when MiddlewareModuleInstance is dropped.
    pub unsafe fn destroy(mut self) {
        tracing::info!(target: "module_loader", "Destroying module instance");
        if let (Some(destroy_fn), Some(ptr)) = (self.destroy.take(), self.ptr.take()) {
            unsafe { destroy_fn(ptr) };
        }
        // Library will be dropped automatically
    }

    /// Convert internal pointer into BoxedMiddleware (Arc<dyn HttpWardMiddleware>).
    /// This consumes self but intentionally *does not* call destroy() here because we've converted
    /// the Box into an Arc and we want Rust to manage the lifetime.
    ///
    /// Implementation approach:
    /// - Reconstruct Box<dyn HttpWardMiddleware> from raw pointer
    /// - Convert Box -> Arc
    pub fn into_boxed_middleware(mut self) -> Arc<dyn HttpWardMiddleware + Send + Sync> {
        unsafe {
            // Take the ptr before consuming self
            let ptr = self.ptr.take().expect("ptr should be available");
            
            // Reconstruct fat pointer from components
            let raw = std::mem::transmute::<(*mut std::ffi::c_void, *mut std::ffi::c_void), *mut (dyn HttpWardMiddleware + Send + Sync)>((ptr.data, ptr.vtable));
            // Reconstruct Box<dyn HttpWardMiddleware + Send + Sync>
            let boxed: Box<dyn HttpWardMiddleware + Send + Sync> = Box::from_raw(raw);
            let arc: Arc<dyn HttpWardMiddleware + Send + Sync> = Arc::from(boxed);

            // Library lifetime is managed by ModuleRecord
            arc
        }
    }
}

impl Drop for MiddlewareModuleInstance {
    fn drop(&mut self) {
        unsafe {
            tracing::info!(target: "module_loader", "Dropping MiddlewareModuleInstance, destroying middleware instance");
            if let (Some(destroy_fn), Some(ptr)) = (self.destroy.take(), self.ptr.take()) {
                destroy_fn(ptr);
            }
        }
    }
}
