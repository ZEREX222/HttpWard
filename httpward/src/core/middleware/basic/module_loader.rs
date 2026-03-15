// httpward/src/core/middleware/basic/module_loader
// Module loader using libloading and raw pointers.
// Comments/in-code text in English.

use std::path::Path;
use std::sync::Arc;
use std::error::Error;
use std::os::raw::c_char;
use libloading::Library;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::pipe::MiddlewareFatPtr;
use httpward_core::module_logging::host_functions::*;

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


/// A loaded module.
/// Keeps the `Library` alive as long as module is used.
pub struct LoadedModule {
    lib: Option<Library>,
    destroy: Option<DestroyFn>,
    ptr: Option<MiddlewareFatPtr>,
}

impl LoadedModule {
    /// Load module library and create middleware instance.
    /// Safety: host and module must be built with the same Rust toolchain and matching core crate types.
    pub unsafe fn load(path: &Path) -> Result<Self, Box<dyn Error + Send + Sync>> {
        tracing::info!(target: "module_loader", "Loading module library from: {}", path.display());
        let lib = unsafe { Library::new(path)? };
        tracing::info!(target: "module_loader", "Library loaded, getting function symbols");
        // Set host loggers in the module
        let set_logger: libloading::Symbol<SetLoggerFn> = unsafe { lib.get(b"module_set_logger")? };
        unsafe { set_logger(host_log_error, host_log_warn, host_log_info, host_log_debug, host_log_trace) };
        tracing::info!(target: "module_loader", "Module loggers set");
        // get symbols
        let create: libloading::Symbol<CreateFn> = unsafe { lib.get(b"create_middleware")? };
        let destroy: libloading::Symbol<DestroyFn> = unsafe { lib.get(b"destroy_middleware")? };
        tracing::info!(target: "module_loader", "Function symbols obtained, creating middleware instance");
        let ptr = unsafe { create() };
        tracing::info!(target: "module_loader", "Middleware instance created successfully");
        // Copy the function pointers before moving lib
        let destroy_fn = *destroy;
        Ok(Self { lib: Some(lib), destroy: Some(destroy_fn), ptr: Some(ptr) })
    }

    /// Manually destroy the module and free resources.
    /// This is called automatically when LoadedModule is dropped.
    pub unsafe fn destroy(mut self) {
        tracing::info!(target: "module_loader", "Destroying module instance");
        if let (Some(destroy_fn), Some(ptr)) = (self.destroy.take(), self.ptr.take()) {
            unsafe { destroy_fn(ptr) };
        }
        // Library will be dropped automatically
    }

    /// Convert internal pointer into BoxedMiddleware (Arc<dyn HttpWardMiddleware>).
    /// This consumes self but intentionally *does not* call destroy() here because we've converted
    /// the Box into an Arc and we want Rust to manage the lifetime. However we still keep the library
    /// alive by moving `lib` into the returned Arc's drop guard if needed.
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

            // Forget the lib to keep it loaded
            if let Some(lib) = self.lib.take() {
                std::mem::forget(lib);
            }
            arc
        }
    }
}

impl Drop for LoadedModule {
    fn drop(&mut self) {
        unsafe {
            tracing::info!(target: "module_loader", "Dropping LoadedModule, destroying middleware instance");
            if let (Some(destroy_fn), Some(ptr)) = (self.destroy.take(), self.ptr.take()) {
                destroy_fn(ptr);
            }
        }
    }
}
