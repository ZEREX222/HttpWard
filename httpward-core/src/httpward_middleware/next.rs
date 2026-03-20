// httpward-core/src/httpward_middleware/next.rs

use std::sync::Arc;
use crate::httpward_middleware::types::BoxError;
use crate::httpward_middleware::middleware_trait::HttpWardMiddleware;
use rama::http::{Body, Request, Response};
use rama::Context;
use crate::httpward_middleware::types::BoxService;

pub struct Next<'a> {
    middlewares: &'a [Arc<dyn HttpWardMiddleware>],
    index: usize,
    inner: &'a BoxService,
}

impl<'a> Next<'a> {
    pub fn new(
        middlewares: &'a [Arc<dyn HttpWardMiddleware>],
        inner: &'a BoxService,
    ) -> Self {
        Self {
            middlewares,
            index: 0,
            inner,
        }
    }

    fn advance(&self) -> Self {
        Self {
            middlewares: self.middlewares,
            index: self.index + 1,
            inner: self.inner,
        }
    }

    pub async fn run(
        self,
        ctx: Context<()>,
        req: Request<Body>,
    ) -> Result<Response<Body>, BoxError> {

        if let Some(mw_box) = self.middlewares.get(self.index) {

            let middleware = mw_box.as_ref();
            let next = self.advance();

            middleware.handle(ctx, req, next).await

        } else {

            // Call the inner service as a function since BoxService is a Fn
            (self.inner)(ctx, req).await
        }
    }
}
