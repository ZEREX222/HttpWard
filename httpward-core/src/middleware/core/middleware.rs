use hyper::{Request, Response, body::Incoming};
use crate::middleware::core::context::RequestContext;

pub enum MiddlewareResult {
    Next(Request<Incoming>),              // Pass to next middleware
    Respond(Response<hyper::body::Bytes>), // Stop and return this response
}

pub trait Middleware: Send + Sync {
    fn handle(&self, req: Request<Incoming>, ctx: &mut RequestContext) -> MiddlewareResult;
}
