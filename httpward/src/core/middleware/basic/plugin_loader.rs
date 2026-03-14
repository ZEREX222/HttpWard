// httpward/src/core/middleware/basic/plugin_loader.rs
// Plugin loader using libloading and raw pointers.
// Comments/in-code text in English.

use std::path::Path;
use std::sync::Arc;
use std::error::Error;
use libloading::Library;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::pipe::MiddlewareFatPtr;

/// C-ABI types exported by plugin
type CreateFn = unsafe extern "C" fn() -> MiddlewareFatPtr;
type DestroyFn = unsafe extern "C" fn(MiddlewareFatPtr);

/// A loaded plugin.
/// Keeps the `Library` alive as long as plugin is used.
pub struct LoadedPlugin {
    lib: Library,
    ptr: MiddlewareFatPtr,
}

impl LoadedPlugin {
    /// Load plugin library and create middleware instance.
    /// Safety: host and plugin must be built with the same Rust toolchain and matching core crate types.
    pub unsafe fn load(path: &Path) -> Result<Self, Box<dyn Error + Send + Sync>> {
        tracing::info!(target: "plugin_loader", "Loading plugin library from: {}", path.display());
        let lib = unsafe { Library::new(path)? };
        tracing::info!(target: "plugin_loader", "Library loaded, getting function symbols");
        // get symbols
        let create: libloading::Symbol<CreateFn> = unsafe { lib.get(b"create_middleware")? };
        let _destroy: libloading::Symbol<DestroyFn> = unsafe { lib.get(b"destroy_middleware")? };
        tracing::info!(target: "plugin_loader", "Function symbols obtained, creating middleware instance");
        let ptr = unsafe { create() };
        tracing::info!(target: "plugin_loader", "Middleware instance created successfully");
        Ok(Self { lib, ptr })
    }

    /// Convert internal pointer into BoxedMiddleware (Arc<dyn HttpWardMiddleware>).
    /// This consumes self but intentionally *does not* call destroy() here because we've converted
    /// the Box into an Arc and we want Rust to manage the lifetime. However we still keep the library
    /// alive by moving `lib` into the returned Arc's drop guard if needed.
    ///
    /// Implementation approach:
    /// - Reconstruct Box<dyn HttpWardMiddleware> from raw pointer
    /// - Convert Box -> Arc
    pub fn into_boxed_middleware(self) -> Arc<dyn HttpWardMiddleware + Send + Sync> {
        unsafe {
            // Reconstruct fat pointer from components
            let raw = std::mem::transmute::<(*mut std::ffi::c_void, *mut std::ffi::c_void), *mut (dyn HttpWardMiddleware + Send + Sync)>((self.ptr.data, self.ptr.vtable));
            // Reconstruct Box<dyn HttpWardMiddleware + Send + Sync>
            let boxed: Box<dyn HttpWardMiddleware + Send + Sync> = Box::from_raw(raw);
            let arc: Arc<dyn HttpWardMiddleware + Send + Sync> = Arc::from(boxed);

            // Forget the lib to keep it loaded
            let lib = self.lib;
            std::mem::forget(lib);
            arc
        }
    }
}
