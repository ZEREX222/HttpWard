use rama::{
    layer::Layer,
    service::Service,
    Context,
};
use std::fmt::Debug;
use tracing::{info, trace};

use crate::middleware::core::{ContentType, HttpWardContext};

/// Layer that adds request logging with custom context
#[derive(Clone, Debug)]
pub struct LogLayer;

impl LogLayer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LogLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for LogLayer {
    type Service = LogService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        LogService::new(inner)
    }
}

/// Service that logs requests and responses with custom context
#[derive(Clone, Debug)]
pub struct LogService<S> {
    inner: S,
}

impl<S> LogService<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S, State, Request> Service<State, Request> for LogService<S>
where
    S: Service<State, Request>,
    Request: Debug,
    S::Response: Debug,
    S::Error: Debug,
    State: Clone + Send + Sync + 'static,
    Request: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;

    async fn serve(
        &self,
        ctx: Context<State>,
        request: Request,
    ) -> Result<Self::Response, Self::Error> {
        // Access request context
        let client_addr = ctx.get::<HttpWardContext>().map(|rc| rc.client_addr);
        let content_type = ctx.get::<HttpWardContext>()
            .map(|rc| rc.request_content_type)
            .unwrap_or(ContentType::Unknown);

        trace!("incoming request");
        info!(
            "[{}] {:?} Request: {:?}",
            client_addr.map_or("unknown".to_string(), |a| a.to_string()),
            content_type,
            request
        );

        let start = std::time::Instant::now();
        let result = self.inner.serve(ctx, request).await;
        let elapsed = start.elapsed();

        match &result {
            Ok(response) => {
                info!("Response: {:?} (took {:?})", response, elapsed);
            }
            Err(error) => {
                info!("Error: {:?} (took {:?})", error, elapsed);
            }
        }

        result
    }
}
