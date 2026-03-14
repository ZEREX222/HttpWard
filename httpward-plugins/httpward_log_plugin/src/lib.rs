// httpward-plugins/httpward_log_plugin/src/lib.rs
use std::os::raw::c_void;
use std::boxed::Box;
use std::sync::Arc;
use async_trait::async_trait;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use httpward_core::httpward_middleware::types::BoxError;
use httpward_core::httpward_middleware::next::Next;
use httpward_core::httpward_middleware::pipe::MiddlewareFatPtr;
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
        println!("[plugin] incoming: {}", req.uri());
        let res = next.run(ctx, req).await?;
        println!("[plugin] outgoing: {}", res.status());
        Ok(res)
    }

    fn name(&self) -> Option<&'static str> {
        Some("DummyLogMiddleware")
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn create_middleware() -> MiddlewareFatPtr {
    println!("[plugin] create_middleware called");
    let boxed: Box<dyn HttpWardMiddleware + Send + Sync> = Box::new(DummyLogMiddleware::new());
    let raw = Box::into_raw(boxed);
    let (data, vtable) = unsafe { std::mem::transmute::<*mut (dyn HttpWardMiddleware + Send + Sync), (*mut c_void, *mut c_void)>(raw) };
    println!("[plugin] middleware created successfully");
    MiddlewareFatPtr { data, vtable }
}

#[unsafe(no_mangle)]
pub extern "C" fn destroy_middleware(ptr: MiddlewareFatPtr) {
    println!("[plugin] destroy_middleware called");
    // If either part is null, nothing to do.
    if ptr.data.is_null() || ptr.vtable.is_null() {
        println!("[plugin] destroy_middleware: null ptr, skipping");
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

    println!("[plugin] destroyed middleware");
}
