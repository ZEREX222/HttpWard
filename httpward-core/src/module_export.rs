// httpward-core/src/module_export.rs
// Generic module export utilities for HttpWard dynamic modules
// Provides reusable export functions to eliminate boilerplate in module implementations

use crate::httpward_middleware::middleware_trait::HttpWardMiddleware;
use crate::httpward_middleware::pipe::MiddlewareFatPtr;
use crate::module_logging::ModuleLogger;
use crate::module_logging::module_setup;
use std::boxed::Box;
use std::os::raw::c_void;

/// Generic module logger setup function
/// This can be used directly by modules or through the export_middleware_module macro
/// Note: This function is not FFI-safe due to &str parameter, use the macro instead
/// Generic middleware creation function
/// Creates a middleware instance of type T and returns it as a fat pointer
///
/// # Safety
/// The returned fat pointer must be passed back to `generic_destroy_middleware` exactly once.
/// Calling code must not mutate or free the returned pointer data manually.
pub unsafe extern "C" fn generic_create_middleware<T>() -> MiddlewareFatPtr
where
    T: HttpWardMiddleware + Send + Sync + 'static,
    T: Default,
{
    let logger = module_setup::get_logger();
    logger.info("generic_create_middleware called");
    logger.debug(&format!(
        "creating new {} instance",
        std::any::type_name::<T>()
    ));

    let boxed: Box<dyn HttpWardMiddleware + Send + Sync> = Box::new(T::default());
    let raw = Box::into_raw(boxed);
    let (data, vtable) = unsafe {
        std::mem::transmute::<*mut (dyn HttpWardMiddleware + Send + Sync), (*mut c_void, *mut c_void)>(
            raw,
        )
    };

    logger.info("middleware created successfully");
    logger.trace("middleware fat pointer created");
    MiddlewareFatPtr { data, vtable }
}

/// Generic middleware destruction function
/// Safely destroys a middleware instance created by generic_create_middleware
///
/// # Safety
/// `ptr` must be a valid pointer pair returned by `generic_create_middleware` from the
/// same module binary, and it must not be used after this call.
pub unsafe extern "C" fn generic_destroy_middleware(ptr: MiddlewareFatPtr) {
    let logger = module_setup::get_logger();
    logger.info("generic_destroy_middleware called");

    // If either part is null, nothing to do.
    if ptr.data.is_null() || ptr.vtable.is_null() {
        logger.warn("generic_destroy_middleware: null ptr, skipping");
        return;
    }

    unsafe {
        // Reconstruct a raw fat pointer *mut (dyn Trait)
        // Transmute the tuple (data, vtable) back into a trait object pointer.
        let raw = std::mem::transmute::<
            (*mut std::ffi::c_void, *mut std::ffi::c_void),
            *mut (dyn HttpWardMiddleware + Send + Sync),
        >((ptr.data, ptr.vtable));

        // Recreate the Box and drop it here inside the module.
        // This ensures free happens in the module's allocator.
        let _boxed: Box<dyn HttpWardMiddleware + Send + Sync> = Box::from_raw(raw);
        // when `_boxed` goes out of scope, it will be dropped here in the module
    }

    logger.info("destroyed middleware");
}

/// Macro to generate all required module export functions
/// This eliminates boilerplate code for new modules
///
/// # Usage Options
///
/// ## 1. Automatic module name (recommended)
/// ```rust
/// use httpward_core::export_middleware_module;
/// use httpward_core::httpward_middleware::context::HttpwardMiddlewareContext;
/// use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
/// use httpward_core::httpward_middleware::next::Next;
/// use rama::{Context, http::{Body, Request, Response}};
/// use async_trait::async_trait;
///
/// #[derive(Default)]
/// struct MyMiddleware;
///
/// #[async_trait]
/// impl HttpWardMiddleware for MyMiddleware {
///     fn name(&self) -> Option<&'static str> {
///         Some("my_middleware")
///     }
///
///     async fn handle(
///         &self,
///         _ctx: &mut HttpwardMiddlewareContext,
///         request: Request<Body>,
///         next: Next<'_>,
///     ) -> Result<Response<Body>, Box<dyn std::error::Error + Send + Sync>> {
///         next.run(_ctx, request).await
///     }
/// }
///
/// export_middleware_module!("my_module", MyMiddleware);  // Use explicit name for doctest
/// ```
///
/// ## 2. Custom module name
/// ```rust
/// use httpward_core::export_middleware_module;
/// use httpward_core::httpward_middleware::context::HttpwardMiddlewareContext;
/// use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
/// use httpward_core::httpward_middleware::next::Next;
/// use rama::{Context, http::{Body, Request, Response}};
/// use async_trait::async_trait;
///
/// #[derive(Default)]
/// struct MyMiddleware;
///
/// #[async_trait]
/// impl HttpWardMiddleware for MyMiddleware {
///     fn name(&self) -> Option<&'static str> {
///         Some("my_middleware")
///     }
///
///     async fn handle(
///         &self,
///         _ctx: &mut HttpwardMiddlewareContext,
///         request: Request<Body>,
///         next: Next<'_>,
///     ) -> Result<Response<Body>, Box<dyn std::error::Error + Send + Sync>> {
///         next.run(_ctx, request).await
///     }
/// }
///
/// export_middleware_module!("custom_name", MyMiddleware);
/// ```
///
/// ## 3. Environment variable name (example with literal)
/// ```rust
/// use httpward_core::export_middleware_module;
/// use httpward_core::httpward_middleware::context::HttpwardMiddlewareContext;
/// use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
/// use httpward_core::httpward_middleware::next::Next;
/// use rama::{Context, http::{Body, Request, Response}};
/// use async_trait::async_trait;
///
/// #[derive(Default)]
/// struct MyMiddleware;
///
/// #[async_trait]
/// impl HttpWardMiddleware for MyMiddleware {
///     fn name(&self) -> Option<&'static str> {
///         Some("my_middleware")
///     }
///
///     async fn handle(
///         &self,
///         _ctx: &mut HttpwardMiddlewareContext,
///         request: Request<Body>,
///         next: Next<'_>,
///     ) -> Result<Response<Body>, Box<dyn std::error::Error + Send + Sync>> {
///         next.run(_ctx, request).await
///     }
/// }
///
/// export_middleware_module!("my_module_name", MyMiddleware);
/// ```
///
/// # Generated Functions
/// - `module_set_logger` - Sets up module logger with the given name
/// - `create_middleware` - Creates middleware instance of type T
/// - `destroy_middleware` - Destroys middleware instance
#[macro_export]
macro_rules! export_middleware_module {
    // Case 1: Only middleware type - auto-detect name from Cargo.toml
    ($middleware_type:ty) => {
        $crate::export_middleware_module!(env: "CARGO_PKG_NAME", $middleware_type);
    };

    // Case 2: Environment variable + middleware type
    (env: $env_var:literal, $middleware_type:ty) => {
        $crate::export_middleware_module!(
            env!($env_var),
            $middleware_type
        );
    };

    // Case 3: Explicit name + middleware type
    ($module_name:expr, $middleware_type:ty) => {
        $crate::paste::paste! {
            #[unsafe(no_mangle)]
            pub extern "C" fn [<$module_name _module_set_logger>](
                error_fn: $crate::module_logging::HostLogErrorFn,
                warn_fn: $crate::module_logging::HostLogWarnFn,
                info_fn: $crate::module_logging::HostLogInfoFn,
                debug_fn: $crate::module_logging::HostLogDebugFn,
                trace_fn: $crate::module_logging::HostLogTraceFn,
            ) {
                $crate::module_logging::module_setup::setup_module_logger_with_name(
                    $module_name,
                    error_fn,
                    warn_fn,
                    info_fn,
                    debug_fn,
                    trace_fn,
                );
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [<$module_name _create_middleware>]() -> $crate::httpward_middleware::pipe::MiddlewareFatPtr {
                unsafe {
                    $crate::module_export::generic_create_middleware::<$middleware_type>()
                }
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [<$module_name _destroy_middleware>](ptr: $crate::httpward_middleware::pipe::MiddlewareFatPtr) {
                unsafe {
                    $crate::module_export::generic_destroy_middleware(ptr)
                }
            }
        }
    };
}

/// Alternative macro for modules that need custom middleware creation logic
/// This provides the logger setup but allows custom create/destroy functions
///
/// # Usage Options
///
/// ## 1. Automatic module name (recommended)
/// ```rust
/// use httpward_core::export_module_with_custom_middleware;
///
/// export_module_with_custom_middleware!("my_module");  // Use explicit name for doctest
/// ```
///
/// ## 2. Custom module name
/// ```rust
/// use httpward_core::export_module_with_custom_middleware;
///
/// export_module_with_custom_middleware!("custom_name");
/// ```
///
/// ## 3. Environment variable name (example with literal)
/// ```rust
/// use httpward_core::export_module_with_custom_middleware;
///
/// export_module_with_custom_middleware!("my_module_name");
/// ```
#[macro_export]
macro_rules! export_module_with_custom_middleware {
    // Case 1: No arguments - auto-detect name from Cargo.toml
    () => {
        $crate::export_module_with_custom_middleware!(env: "CARGO_PKG_NAME");
    };

    // Case 2: Environment variable
    (env: $env_var:literal) => {
        $crate::export_module_with_custom_middleware!(
            env!($env_var)
        );
    };

    // Case 3: Explicit name
    ($module_name:expr) => {
        $crate::paste::paste! {
            #[unsafe(no_mangle)]
            pub extern "C" fn [<$module_name _module_set_logger>](
                error_fn: $crate::module_logging::HostLogErrorFn,
                warn_fn: $crate::module_logging::HostLogWarnFn,
                info_fn: $crate::module_logging::HostLogInfoFn,
                debug_fn: $crate::module_logging::HostLogDebugFn,
                trace_fn: $crate::module_logging::HostLogTraceFn,
            ) {
                $crate::module_logging::module_setup::setup_module_logger_with_name(
                    $module_name,
                    error_fn,
                    warn_fn,
                    info_fn,
                    debug_fn,
                    trace_fn,
                );
            }

            // Note: You must provide your own [<$module_name _create_middleware>] and [<$module_name _destroy_middleware>] functions
        }
    };
}

/// Helper trait for middleware that can be created with default constructor
/// This is used by the generic_create_middleware function
pub trait DefaultMiddleware: HttpWardMiddleware + Send + Sync + 'static {
    fn create_default() -> Self
    where
        Self: Sized;
}

impl<T> DefaultMiddleware for T
where
    T: HttpWardMiddleware + Send + Sync + 'static + Default,
{
    fn create_default() -> Self
    where
        Self: Sized,
    {
        T::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::httpward_middleware::BoxError;
    use crate::httpward_middleware::context::HttpwardMiddlewareContext;
    use crate::httpward_middleware::middleware_trait::HttpWardMiddleware;
    use crate::httpward_middleware::next::Next;
    use rama::http::{Body, Request, Response};

    // Test middleware for testing purposes
    #[derive(Debug, Default)]
    struct TestMiddleware;

    #[async_trait::async_trait]
    impl HttpWardMiddleware for TestMiddleware {
        fn name(&self) -> Option<&'static str> {
            Some("test_middleware")
        }

        async fn handle(
            &self,
            ctx: &mut HttpwardMiddlewareContext,
            request: Request<Body>,
            next: Next<'_>,
        ) -> Result<Response<Body>, BoxError> {
            next.run(ctx, request).await
        }
    }

    #[test]
    fn test_generic_create_destroy_middleware() {
        // Test that we can create and destroy middleware safely
        let ptr = unsafe { generic_create_middleware::<TestMiddleware>() };

        assert!(!ptr.data.is_null(), "Data pointer should not be null");
        assert!(!ptr.vtable.is_null(), "VTable pointer should not be null");

        unsafe { generic_destroy_middleware(ptr) };
    }

    #[test]
    fn test_export_macro_compilation() {
        // Test that the macro compiles correctly
        // This is a compile-time test
        export_middleware_module!("test_module", TestMiddleware);
    }
}
