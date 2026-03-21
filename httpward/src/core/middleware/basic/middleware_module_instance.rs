// httpward/src/core/middleware/basic/module_loader
// Module loader using libloading and raw pointers.
// Comments/in-code text in English.

use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::pipe::MiddlewareFatPtr;
use httpward_core::module_logging::host_functions::*;
use libloading::Library;
use std::os::raw::c_char;
use std::sync::Arc;
use std::sync::Mutex;

/// Global mutex for thread-safe library loading
static LIBRARY_LOADING_MUTEX: Mutex<()> = Mutex::new(());

/// C-ABI types exported by module
type CreateFn = unsafe extern "C" fn() -> MiddlewareFatPtr;
type DestroyFn = unsafe extern "C" fn(MiddlewareFatPtr);
type SetLoggerFn = unsafe extern "C" fn(
    extern "C" fn(*const c_char), // error
    extern "C" fn(*const c_char), // warn
    extern "C" fn(*const c_char), // info
    extern "C" fn(*const c_char), // debug
    extern "C" fn(*const c_char), // trace
);

/// Collected module exports required by the host.
#[derive(Copy, Clone)]
struct ModuleExports {
    create: CreateFn,
    destroy: DestroyFn,
    set_logger: SetLoggerFn,
}

/// A middleware module instance.
/// Keeps the `Library` alive as long as module is used.
pub struct MiddlewareModuleInstance {
    lib: Arc<Library>,
    destroy: Option<DestroyFn>,
    ptr: Option<MiddlewareFatPtr>,
}

impl MiddlewareModuleInstance {
    /// Load required symbols from a shared library.
    ///
    /// # Safety
    /// Host and module must agree on symbol names and ABI.
    unsafe fn load_exports_unchecked(
        lib: &Library,
    ) -> Result<ModuleExports, Box<dyn std::error::Error + Send + Sync>> {
        // SAFETY: Symbol lookup relies on module ABI contract and exact symbol names.
        let set_logger: libloading::Symbol<SetLoggerFn> = unsafe { lib.get(b"module_set_logger")? };
        // SAFETY: Symbol lookup relies on module ABI contract and exact symbol names.
        let create: libloading::Symbol<CreateFn> = unsafe { lib.get(b"create_middleware")? };
        // SAFETY: Symbol lookup relies on module ABI contract and exact symbol names.
        let destroy: libloading::Symbol<DestroyFn> = unsafe { lib.get(b"destroy_middleware")? };

        Ok(ModuleExports {
            create: *create,
            destroy: *destroy,
            set_logger: *set_logger,
        })
    }

    /// Rebuild Arc<dyn HttpWardMiddleware> from module fat pointer.
    ///
    /// # Safety
    /// `ptr` must be produced by `create_middleware` for the same trait layout expected by host.
    unsafe fn fat_ptr_into_arc_unchecked(
        ptr: MiddlewareFatPtr,
    ) -> Arc<dyn HttpWardMiddleware + Send + Sync> {
        // SAFETY: The pointer pair is expected to be a valid trait-object fat pointer from module ABI.
        let raw = unsafe {
            std::mem::transmute::<
                (*mut std::ffi::c_void, *mut std::ffi::c_void),
                *mut (dyn HttpWardMiddleware + Send + Sync),
            >((ptr.data, ptr.vtable))
        };
        // SAFETY: Ownership of the allocation is transferred from module factory into host-managed Box.
        let boxed: Box<dyn HttpWardMiddleware + Send + Sync> = unsafe { Box::from_raw(raw) };
        Arc::from(boxed)
    }

    /// Create middleware instance from Arc<Library>.
    /// Safety: host and module must agree on ABI.
    pub unsafe fn create_from_arc(
        lib: Arc<Library>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Ensure thread-safe symbol loading and module initialization sequence.
        let _guard = LIBRARY_LOADING_MUTEX
            .lock()
            .map_err(|e| format!("Failed to acquire module loading lock: {e}"))?;

        tracing::info!(target: "module_loader", "Creating middleware instance from shared library");
        tracing::info!(target: "module_loader", "Getting function symbols from library");

        // SAFETY: Export lookup depends on ABI compatibility between host and module.
        let exports = unsafe { Self::load_exports_unchecked(&lib) }?;
        // SAFETY: Host logging callbacks follow module_set_logger contract.
        unsafe {
            (exports.set_logger)(
                host_log_error,
                host_log_warn,
                host_log_info,
                host_log_debug,
                host_log_trace,
            )
        };
        tracing::info!(target: "module_loader", "Module loggers set");

        tracing::info!(target: "module_loader", "Function symbols obtained, creating middleware instance");
        // SAFETY: create function pointer is loaded from module exports and follows CreateFn ABI.
        let ptr = unsafe { (exports.create)() };
        tracing::info!(target: "module_loader", "Middleware instance created successfully");

        Ok(Self {
            lib,
            destroy: Some(exports.destroy),
            ptr: Some(ptr),
        })
    }

    /// Manually destroy the module and free resources.
    /// This is called automatically when MiddlewareModuleInstance is dropped.
    pub unsafe fn destroy(mut self) {
        tracing::info!(target: "module_loader", "Destroying module instance");
        if let (Some(destroy_fn), Some(ptr)) = (self.destroy.take(), self.ptr.take()) {
            unsafe { destroy_fn(ptr) };
        }
        // Library is kept alive by self.lib and will be dropped automatically.
    }

    /// Convert internal pointer into BoxedMiddleware (Arc<dyn HttpWardMiddleware>).
    /// This consumes self but intentionally *does not* call destroy() here because we've converted
    /// the Box into an Arc and we want Rust to manage the lifetime.
    ///
    /// Implementation approach:
    /// - Reconstruct Box<dyn HttpWardMiddleware> from raw pointer
    /// - Convert Box -> Arc
    pub fn into_boxed_middleware(mut self) -> Arc<dyn HttpWardMiddleware + Send + Sync> {
        // Keep library alive inside this struct until conversion is complete.
        let _lib_guard = &self.lib;
        // Take the ptr before consuming self.
        let ptr = self.ptr.take().expect("ptr should be available");

        // SAFETY: Pointer is produced by create_middleware and consumed exactly once here.
        unsafe { Self::fat_ptr_into_arc_unchecked(ptr) }
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
