// File: httpward-core/src/httpward_middleware/types.rs

use std::error::Error;
use std::pin::Pin;
use std::future::Future;
use rama::http::{Body, Request, Response};

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
