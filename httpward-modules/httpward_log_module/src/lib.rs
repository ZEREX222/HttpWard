// httpward-modules/httpward_log_module/src/lib.rs
use std::os::raw::c_void;
use std::boxed::Box;
use async_trait::async_trait;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::types::BoxError;
use httpward_core::httpward_middleware::next::Next;
use httpward_core::httpward_middleware::pipe::MiddlewareFatPtr;
use httpward_core::module_logging::{HostLogErrorFn, HostLogWarnFn, HostLogInfoFn, HostLogDebugFn, HostLogTraceFn, ModuleLogger};
use httpward_core::module_logging::module_setup;
use rama::http::{Body, Request, Response};
use rama::Context;
use std::fmt;

#[derive(Clone)]
struct DummyLogMiddleware {
    tag: Option<String>,
}

impl DummyLogMiddleware {
    fn new() -> Self {
        Self { tag: Some("plugin-dummy".to_string()) }
    }
}

impl fmt::Debug for DummyLogMiddleware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DummyLogMiddleware").field("tag", &self.tag).finish()
    }
}

#[async_trait]
impl HttpWardMiddleware for DummyLogMiddleware {
    async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
        let logger = module_setup::get_logger();
        logger.info(&format!("incoming: {}", req.uri()));
        logger.debug(&format!("request headers: {:?}", req.headers()));
        logger.trace(&format!("request method: {}", req.method()));
        
        // Example of warning condition
        if req.uri().path().contains("/admin") {
            logger.warn("Admin access detected");
        }
        
        // Example of error condition
        if req.uri().path().contains("/error") {
            logger.error("Error endpoint accessed - this is a test error");
        }
        
        let res = next.run(ctx, req).await?;
        logger.info(&format!("outgoing: {}", res.status()));
        logger.debug(&format!("response headers: {:?}", res.headers()));
        logger.trace(&format!("response version: {:?}", res.version()));
        Ok(res)
    }

    fn name(&self) -> Option<&'static str> {
        Some("DummyLogMiddleware")
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn module_set_logger(
    error_fn: HostLogErrorFn,
    warn_fn: HostLogWarnFn, 
    info_fn: HostLogInfoFn,
    debug_fn: HostLogDebugFn,
    trace_fn: HostLogTraceFn,
) {
    unsafe {
        module_setup::setup_module_logger_with_name("log_module", error_fn, warn_fn, info_fn, debug_fn, trace_fn);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn create_middleware() -> MiddlewareFatPtr {
    let logger = module_setup::get_logger();
    logger.info("create_middleware called");
    logger.debug("creating new DummyLogMiddleware instance");
    let boxed: Box<dyn HttpWardMiddleware + Send + Sync> = Box::new(DummyLogMiddleware::new());
    let raw = Box::into_raw(boxed);
    let (data, vtable) = unsafe { std::mem::transmute::<*mut (dyn HttpWardMiddleware + Send + Sync), (*mut c_void, *mut c_void)>(raw) };
    logger.info("middleware created successfully");
    logger.trace("middleware fat pointer created");
    MiddlewareFatPtr { data, vtable }
}

#[unsafe(no_mangle)]
pub extern "C" fn destroy_middleware(ptr: MiddlewareFatPtr) {
    let logger = module_setup::get_logger();
    logger.info("destroy_middleware called");
    // If either part is null, nothing to do.
    if ptr.data.is_null() || ptr.vtable.is_null() {
        logger.warn("destroy_middleware: null ptr, skipping");
        return;
    }

    unsafe {
        // Reconstruct a raw fat pointer *mut (dyn Trait)
        // Transmute the tuple (data, vtable) back into a trait object pointer.
        let raw = std::mem::transmute::<(*mut std::ffi::c_void, *mut std::ffi::c_void), *mut (dyn HttpWardMiddleware + Send + Sync)>( (ptr.data, ptr.vtable) );

        // Recreate the Box and drop it here inside the module.
        // This ensures free happens in the module's allocator.
        let _boxed: Box<dyn HttpWardMiddleware + Send + Sync> = Box::from_raw(raw);
        // when `_boxed` goes out of scope, it will be dropped here in the module
    }

    logger.info("destroyed middleware");
}
