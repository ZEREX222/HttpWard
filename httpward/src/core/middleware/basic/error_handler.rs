use rama::{
    layer::Layer,
    service::Service,
    Context,
    http::{Request as RamaRequest, Response as RamaResponse, Body as RamaBody, StatusCode},
};
use std::fmt::Debug;
use tracing::error;
use crate::core::error::ErrorHandler;

/// Layer that provides consistent error handling
#[derive(Clone, Debug)]
pub struct ErrorHandlerLayer {
    error_handler: ErrorHandler,
}

impl ErrorHandlerLayer {
    pub fn new() -> Self {
        Self {
            error_handler: ErrorHandler::default(),
        }
    }
}

impl Default for ErrorHandlerLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for ErrorHandlerLayer {
    type Service = ErrorHandlerService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ErrorHandlerService::new(inner, self.error_handler.clone())
    }
}

/// Service that provides consistent error handling
#[derive(Clone, Debug)]
pub struct ErrorHandlerService<S> {
    inner: S,
    error_handler: ErrorHandler,
}

impl<S> ErrorHandlerService<S> {
    pub fn new(inner: S, error_handler: ErrorHandler) -> Self {
        Self { inner, error_handler }
    }
}

impl<S, State, Request> Service<State, Request> for ErrorHandlerService<S>
where
    S: Service<State, Request, Response = RamaResponse<RamaBody>>,
    S::Error: Debug,
    State: Clone + Send + Sync + 'static,
    Request: Send + 'static,
{
    type Response = RamaResponse<RamaBody>;
    type Error = S::Error;

    async fn serve(
        &self,
        ctx: Context<State>,
        request: Request,
    ) -> Result<Self::Response, Self::Error> {
        match self.inner.serve(ctx, request).await {
            Ok(response) => Ok(response),
            Err(e) => {
                error!("Service error: {:?}", e);
                // Convert any service error to a proper HTTP error response
                let error_response = self.error_handler.create_error_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
                    .unwrap_or_else(|_| RamaResponse::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(RamaBody::from("Internal server error"))
                        .unwrap());
                Ok(error_response)
            }
        }
    }
}
