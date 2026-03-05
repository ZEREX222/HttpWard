//! Custom Middleware Framework for HttpWard
//!
//! This module provides a convenient way to implement custom middleware using Rama's Layer/Service pattern.
//!
//! ## Creating Custom Middleware
//!
//! To create a custom middleware, implement two components:
//!
//! 1. **Layer** - The factory that creates the middleware service
//! 2. **Service** - The actual middleware logic
//!
//! ### Example:
//!
//! ```rust
//! use rama::{ layer::Layer, service::Service, Context };
//! use httpward_core::middleware::{HttpWardContext, prelude::*};
//!
//! #[derive(Clone, Debug)]
//! pub struct MyLayer { config: String }
//!
//! impl<S> Layer<S> for MyLayer {
//!     type Service = MyService<S>;
//!     fn layer(&self, inner: S) -> Self::Service {
//!         MyService::new(inner, self.config.clone())
//!     }
//! }
//!
//! #[derive(Clone, Debug)]
//! pub struct MyService<S> { inner: S, config: String }
//!
//! impl<S, State, Request> Service<State, Request> for MyService<S>
//! where S: Service<State, Request>, State: Clone + Send + Sync + 'static,
//! {
//!     type Response = S::Response;
//!     type Error = S::Error;
//!
//!     async fn serve(&self, ctx: Context<State>, request: Request)
//!         -> Result<Self::Response, Self::Error> {
//!         // Access request context
//!         if let Some(req_ctx) = ctx.get::<HttpWardContext>() {
//!             println!("Client: {:?}", req_ctx.client_addr);
//!         }
//!         self.inner.serve(ctx, request).await
//!     }
//! }
//! ```

mod context;
mod content_type;

pub use context::{ContentType, HttpWardContext};
pub use content_type::parse_content_type;
pub use crate::middleware::log::{LogLayer, LogService};

/// Re-export core Rama types for middleware development
pub mod rama {
    pub use rama::{ layer::Layer, service::Service, Context };
}

/// Utility functions for middleware development
pub mod utils {
}

/// Prelude for easy imports in middleware modules
pub mod prelude {
    pub use super::{ContentType, HttpWardContext, LogLayer };
    pub use super::rama::*;
}
