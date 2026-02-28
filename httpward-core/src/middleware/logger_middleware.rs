use hyper::{Request, body::Incoming};
use tracing::{debug, info, error};
use std::time::Instant;

use crate::middleware::core::{Middleware, MiddlewareFuture, Next, RequestContext};

pub struct LoggerMiddleware;

impl Middleware for LoggerMiddleware {
    fn handle<'m>(
        &'m self,
        req: Request<Incoming>,
        ctx: &'m mut RequestContext,
        next: Next<'m>,
    ) -> MiddlewareFuture<'m> {
        // We wrap the logic in Box::pin(async move { ... }) 
        // because Middleware defines it as a Pinned Future.
        Box::pin(async move {
            let start = Instant::now();
            let client_ip = ctx.client_addr;
            let method = req.method().clone();
            let path = req.uri().path().to_string();
            let score = ctx.score;

            // --- Phase 1: Inbound Logging ---
            debug!(
                "[INBOUND] {} | {} {} | Score: {}",
                client_ip, method, path, score
            );

            // Execute the rest of the pipeline (other middlewares + backend)
            let result = next(req, ctx).await;

            // --- Phase 2: Outbound Logging (After Backend responds) ---
            let duration = start.elapsed();

            match result {
                Ok(response) => {
                    info!(
                        "[OUTBOUND] {} | {} {} | Status: {} | Time: {:?} | Score: {}",
                        client_ip, 
                        method, 
                        path, 
                        response.status(), 
                        duration,
                        score
                    );
                    Ok(response)
                }
                Err(e) => {
                    // This catches errors from the backend or downstream middlewares
                    error!(
                        "[ERROR] {} | {} {} | Error: {:?} | Time: {:?}",
                        client_ip, method, path, e, duration
                    );
                    Err(e)
                }
            }
        })
    }
}