// File: httpward-core/src/httpward_middleware/types.rs

use rama::http::{Body, Request, Response};
use std::error::Error;
use std::future::Future;
use std::pin::Pin;

/// Unified boxed error type used by middleware chain.
pub type BoxError = Box<dyn Error + Send + Sync>;

/// A type-erased async service function that middleware will call as the final target.
pub type BoxService = Box<
    dyn Fn(
            rama::Context<()>,
            Request<Body>,
        ) -> Pin<Box<dyn Future<Output = Result<Response<Body>, BoxError>> + Send>>
        + Send
        + Sync,
>;
