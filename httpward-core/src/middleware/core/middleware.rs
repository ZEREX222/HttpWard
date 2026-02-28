use std::future::Future;
use std::pin::Pin;
use hyper::{Request, Response, body::Incoming};
use hyper::body::Bytes;
use http_body_util::{Either, Full};

use crate::middleware::core::context::RequestContext;

/// A unified body type for the entire pipeline.
/// Left: A full body in memory (Middleware errors/stubs).
/// Right: A streaming body (Incoming from the network/backend).
pub type BoxBody = Either<Full<Bytes>, Incoming>;

/// A pinned, heap-allocated Future for asynchronous middleware execution.
pub type MiddlewareFuture<'m> = Pin<Box<dyn Future<Output = Result<Response<BoxBody>, hyper::Error>> + Send + 'm>>;

/// A trait for Middleware that supports the "Onion" pattern.
pub trait Middleware: Send + Sync {
    fn handle<'m>(
        &'m self,
        req: Request<Incoming>,
        ctx: &'m mut RequestContext,
        next: Next<'m>,
    ) -> MiddlewareFuture<'m>;
}

/// The 'Next' type represents the rest of the chain (other middlewares + the final backend handler).
pub type Next<'m> = Box<dyn FnOnce(Request<Incoming>, &'m mut RequestContext) -> MiddlewareFuture<'m> + Send + 'm>;
