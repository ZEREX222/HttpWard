// httpward-modules/httpward_log_module/src/lib.rs
use std::os::raw::c_void;
use std::boxed::Box;
use std::ffi::CString;
use std::os::raw::c_char;
use async_trait::async_trait;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::types::BoxError;
use httpward_core::httpward_middleware::next::Next;
use httpward_core::httpward_middleware::pipe::MiddlewareFatPtr;
use rama::http::{Body, Request, Response};
use rama::Context;
use std::fmt;

// Host logging function types with different log levels
type HostLogErrorFn = extern "C" fn(*const c_char);
type HostLogWarnFn = extern "C" fn(*const c_char);
type HostLogInfoFn = extern "C" fn(*const c_char);
type HostLogDebugFn = extern "C" fn(*const c_char);
type HostLogTraceFn = extern "C" fn(*const c_char);

// Global host logging callbacks
static mut HOST_LOG_ERROR: Option<HostLogErrorFn> = None;
static mut HOST_LOG_WARN: Option<HostLogWarnFn> = None;
static mut HOST_LOG_INFO: Option<HostLogInfoFn> = None;
static mut HOST_LOG_DEBUG: Option<HostLogDebugFn> = None;
static mut HOST_LOG_TRACE: Option<HostLogTraceFn> = None;

// Plugin logging functions that call host
fn log_warn(msg: &str) {
    unsafe {
        if let Some(cb) = HOST_LOG_WARN {
            let c = CString::new(msg).unwrap();
            cb(c.as_ptr());
        }
    }
}

fn log_info(msg: &str) {
    unsafe {
        if let Some(cb) = HOST_LOG_INFO {
            let c = CString::new(msg).unwrap();
            cb(c.as_ptr());
        }
    }
}

fn log_debug(msg: &str) {
    unsafe {
        if let Some(cb) = HOST_LOG_DEBUG {
            let c = CString::new(msg).unwrap();
            cb(c.as_ptr());
        }
    }
}

fn log_trace(msg: &str) {
    unsafe {
        if let Some(cb) = HOST_LOG_TRACE {
            let c = CString::new(msg).unwrap();
            cb(c.as_ptr());
        }
    }
}

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
        log_info(&format!("[plugin] incoming: {}", req.uri()));
        log_debug(&format!("[plugin] request headers: {:?}", req.headers()));
        let res = next.run(ctx, req).await?;
        log_info(&format!("[plugin] outgoing: {}", res.status()));
        log_trace(&format!("[plugin] response headers: {:?}", res.headers()));
        Ok(res)
    }

    fn name(&self) -> Option<&'static str> {
        Some("DummyLogMiddleware")
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn plugin_set_logger(
    error_fn: HostLogErrorFn,
    warn_fn: HostLogWarnFn, 
    info_fn: HostLogInfoFn,
    debug_fn: HostLogDebugFn,
    trace_fn: HostLogTraceFn,
) {
    unsafe {
        HOST_LOG_ERROR = Some(error_fn);
        HOST_LOG_WARN = Some(warn_fn);
        HOST_LOG_INFO = Some(info_fn);
        HOST_LOG_DEBUG = Some(debug_fn);
        HOST_LOG_TRACE = Some(trace_fn);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn create_middleware() -> MiddlewareFatPtr {
    log_info("[plugin] create_middleware called");
    log_debug("[plugin] creating new DummyLogMiddleware instance");
    let boxed: Box<dyn HttpWardMiddleware + Send + Sync> = Box::new(DummyLogMiddleware::new());
    let raw = Box::into_raw(boxed);
    let (data, vtable) = unsafe { std::mem::transmute::<*mut (dyn HttpWardMiddleware + Send + Sync), (*mut c_void, *mut c_void)>(raw) };
    log_info("[plugin] middleware created successfully");
    log_trace("[plugin] middleware fat pointer created");
    MiddlewareFatPtr { data, vtable }
}

#[unsafe(no_mangle)]
pub extern "C" fn destroy_middleware(ptr: MiddlewareFatPtr) {
    log_info("[plugin] destroy_middleware called");
    // If either part is null, nothing to do.
    if ptr.data.is_null() || ptr.vtable.is_null() {
        log_warn("[plugin] destroy_middleware: null ptr, skipping");
        return;
    }

    unsafe {
        // Reconstruct a raw fat pointer *mut (dyn Trait)
        // Transmute the tuple (data, vtable) back into a trait object pointer.
        let raw = std::mem::transmute::<(*mut std::ffi::c_void, *mut std::ffi::c_void), *mut (dyn HttpWardMiddleware + Send + Sync)>( (ptr.data, ptr.vtable) );

        // Recreate the Box and drop it here inside the plugin.
        // This ensures free happens in the plugin's allocator.
        let _boxed: Box<dyn HttpWardMiddleware + Send + Sync> = Box::from_raw(raw);
        // when `_boxed` goes out of scope, it will be dropped here in the plugin
    }

    log_info("[plugin] destroyed middleware");
}
