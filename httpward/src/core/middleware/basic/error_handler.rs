use httpward_core::error::ErrorHandler;
use httpward_core::httpward_middleware::IsHttpWardError;
use rama::{
    Context,
    http::{Body as RamaBody, Response as RamaResponse, StatusCode},
    layer::Layer,
    service::Service,
};
use std::fmt::Debug;
use tracing::{error, info, warn};

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
        Self {
            inner,
            error_handler,
        }
    }

    /// Handle HttpWardMiddlewareError specifically
    fn handle_custom_error<E>(&self, error: &E) -> Option<RamaResponse<RamaBody>>
    where
        E: std::any::Any + Send + Sync,
    {
        // Try to downcast to Box<dyn Error + Send + Sync> first
        if let Some(error_box) = (error as &(dyn std::any::Any + Send + Sync))
            .downcast_ref::<Box<dyn std::error::Error + Send + Sync>>()
        {
            // Now try to check if it contains HttpWardMiddlewareError
            if let Some(middleware_error) = error_box.as_httpward_error() {
                let status = middleware_error.status_code();
                let title = middleware_error.title();
                let description = middleware_error.description();

                info!(
                    status = %status.as_u16(),
                    title = %title,
                    description = %description,
                    "Handling HttpWardMiddlewareError"
                );

                return self
                    .error_handler
                    .create_error_response(status, title, description)
                    .ok();
            }
        }

        None
    }

    /// Create a generic error response for non-HttpWard errors
    fn create_generic_error_response(&self) -> RamaResponse<RamaBody> {
        self.error_handler
            .create_error_response_with_code(StatusCode::INTERNAL_SERVER_ERROR)
            .unwrap_or_else(|_| {
                RamaResponse::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(RamaBody::from("Internal server error"))
                    .unwrap()
            })
    }
}

impl<S, State, Request> Service<State, Request> for ErrorHandlerService<S>
where
    S: Service<State, Request, Response = RamaResponse<RamaBody>>,
    S::Error: Debug + Send + Sync,
    State: Clone + Send + Sync + 'static,
    Request: Send + 'static,
{
    type Response = RamaResponse<RamaBody>;
    type Error = std::convert::Infallible;

    async fn serve(
        &self,
        ctx: Context<State>,
        request: Request,
    ) -> Result<Self::Response, Self::Error> {
        match self.inner.serve(ctx, request).await {
            Ok(response) => Ok(response),
            Err(e) => {
                error!("Service error occurred: {:?}", e);

                // Try to extract HttpWardMiddlewareError from the error
                let error_response = self.handle_custom_error(&e).unwrap_or_else(|| {
                    warn!(
                        "Could not extract HttpWardMiddlewareError, using generic error handling"
                    );
                    self.create_generic_error_response()
                });

                Ok(error_response)
            }
        }
    }
}
