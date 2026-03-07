use rama::{
    http::{Request, Response},
    layer::Layer,
    service::Service,
    Context,
};
use std::fmt::Debug;


use rama::http::Body;
use httpward_core::core::{parse_content_type, ContentType, HttpWardContext};

/// Extract content type from response headers (generic version)
fn extract_content_type_from_response_generic<T>(response: &T) -> ContentType 
where
    T: std::any::Any,
{
    // Try to downcast to Response<Body>
    if let Some(http_response) = (response as &dyn std::any::Any).downcast_ref::<Response<Body>>() {
        if let Some(headers) = http_response.headers().get("content-type") {
            if let Ok(content_type_str) = headers.to_str() {
                return parse_content_type(content_type_str);
            }
        }
    }
    ContentType::Unknown
}

/// Layer that enriches response context with response_content_type
#[derive(Clone, Debug)]
pub struct ResponseEnricherLayer;

impl ResponseEnricherLayer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ResponseEnricherLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for ResponseEnricherLayer {
    type Service = ResponseEnricherService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseEnricherService::new(inner)
    }
}

/// Service that enriches responses with response_content_type
#[derive(Clone, Debug)]
pub struct ResponseEnricherService<S> {
    inner: S,
}

impl<S> ResponseEnricherService<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S, State> Service<State, Request<Body>> for ResponseEnricherService<S>
where
    S: Service<State, Request<Body>>,
    S::Response: Debug,
    S::Error: Debug,
    State: Clone + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = S::Error;

    async fn serve(
        &self,
        mut ctx: Context<State>,
        request: Request<Body>,
    ) -> Result<Self::Response, Self::Error> {
        let ctx_clone = ctx.clone();
        let result = self.inner.serve(ctx_clone, request).await;

        if let Ok(response) = &result {
            // Try to extract content type from response headers
            let response_content_type = extract_content_type_from_response_generic(response);
            
            if let Some(hw_ctx) = ctx.get_mut::<HttpWardContext>() {
                hw_ctx.response_content_type = response_content_type;
            }
        }

        result
    }
}
