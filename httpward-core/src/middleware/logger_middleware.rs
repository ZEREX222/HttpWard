use hyper::{Request, body::Incoming};
use tracing::debug;
use crate::middleware::core::{Middleware, MiddlewareResult, RequestContext};

pub struct LoggerMiddleware;

impl Middleware for LoggerMiddleware {
    fn handle(&self, req: Request<Incoming>, ctx: &mut RequestContext) -> MiddlewareResult {
        debug!(
            "[LOG] Request from {} | Path: {} | Score: {}",
            ctx.client_addr,
            req.uri().path(),
            ctx.score
        );

        // Go next
        MiddlewareResult::Next(req)
    }
}
