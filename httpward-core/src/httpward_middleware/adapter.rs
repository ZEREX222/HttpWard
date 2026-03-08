// File: httpward-core/src/httpward_middleware/adapter.rs

use std::pin::Pin;
use std::future::Future;
use crate::httpward_middleware::types::{BoxError, BoxService};
use rama::http::{Body, Request, Response};
use rama::service::Service;
use rama::Context;

/// Helper: convert a concrete S into a BoxService
pub fn box_service_from<S>(svc: S) -> BoxService
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    Box::new(move |ctx: Context<()>, req: Request<Body>| {
        let svc = svc.clone();
        Box::pin(async move {
            svc.serve(ctx, req)
                .await
                .map_err(|e| Box::new(e) as BoxError)
        }) as Pin<Box<dyn Future<Output = Result<Response<Body>, BoxError>> + Send>>
    })
}
