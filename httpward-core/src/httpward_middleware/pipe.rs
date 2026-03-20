// File: httpward-core/src/httpward_middleware/pipe.rs

use std::os::raw::c_void;
use std::sync::Arc;
use crate::httpward_middleware::middleware_trait::HttpWardMiddleware;
use crate::httpward_middleware::next::Next;
use crate::httpward_middleware::types::BoxError;
use crate::httpward_middleware::dependency_error::DependencyError;
use rama::http::{Body, Request, Response};
use rama::Context;
use rama::service::Service;
use std::fmt;

/// Type alias for boxed middleware stored in the internal Vec.
/// Each middleware must be Send + Sync because the Vec will be shared between threads.
type BoxedMiddleware = Arc<dyn HttpWardMiddleware>;

/// Public wrapper around shared pipeline storage.
/// The internal Vec is wrapped in an Arc so cloning the pipe is cheap (one atomic increment).
#[derive(Clone)]
pub struct HttpWardMiddlewarePipe {
    inner: Arc<Vec<BoxedMiddleware>>,
}

impl Default for HttpWardMiddlewarePipe {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for HttpWardMiddlewarePipe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpWardMiddlewarePipe")
            .field("middleware_count", &self.inner.len())
            .finish()
    }
}

impl HttpWardMiddlewarePipe {
    /// Create an empty, cheap-to-clone pipe.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Vec::new()),
        }
    }

    /// Number of middlewares in the pipe.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Add a middleware to the pipe with mandatory dependency validation
    pub fn add_layer<M>(&self, mw: M) -> Result<Self, DependencyError>
    where
        M: HttpWardMiddleware + Send + Sync + 'static,
    {
        let mw_name = mw.name().unwrap_or("unnamed");
        let deps = mw.dependencies();
        
        // Check that all dependencies are already present in pipe
        for &dep in &deps {
            if self.get_layer_by_name(dep).is_none() {
                return Err(DependencyError::MissingDependency {
                    middleware: mw_name.to_string(),
                    dependency: dep.to_string(),
                });
            }
        }
        
        // If all dependencies are present, add middleware
        let mut new_vec = (*self.inner).clone();
        new_vec.push(Arc::new(mw));
        Ok(Self {
            inner: Arc::new(new_vec),
        })
    }

    /// Find layer by name (middleware may return a name via `name()`).
    /// Returns a reference to the boxed middleware if found.
    pub fn get_layer_by_name(
        &self,
        name: &str,
    ) -> Option<&BoxedMiddleware> {
        self.inner.iter().find(|m| m.name().map_or(false, |n| n == name))
    }

    /// Get an iterator over all middleware in the pipe.
    /// Returns a slice of all `BoxedMiddleware` items.
    pub fn iter(&self) -> std::slice::Iter<'_, BoxedMiddleware> {
        self.inner.iter()
    }

    /// Create a new pipe containing **only** the middleware whose names appear in `active_names`.
    /// Middleware without a name (`name() == None`) are always included.
    ///
    /// This is a cheap operation: each `BoxedMiddleware` is an `Arc`, so only Arc-pointers
    /// are cloned — the underlying middleware objects are not copied.
    ///
    /// Typical use: precompute a per-route filtered pipe at startup so that at request time
    /// only the middleware belonging to the active strategy for that route are executed.
    pub fn create_filtered(&self, active_names: &std::collections::HashSet<&str>) -> Self {
        let filtered: Vec<BoxedMiddleware> = self.inner
            .iter()
            .filter(|mw| match mw.name() {
                Some(name) => active_names.contains(name),
                None => true, // unnamed middleware always runs
            })
            .cloned()
            .collect();
        Self {
            inner: Arc::new(filtered),
        }
    }

    /// Add a boxed middleware (Arc<dyn HttpWardMiddleware>) into the pipe with dependency validation.
    /// This is useful when the middleware is created dynamically (plugins).
    pub fn add_boxed_layer(&self, mw: BoxedMiddleware) -> Result<Self, DependencyError> {
        let mw_name = mw.name().unwrap_or("unnamed");
        let deps = mw.dependencies();
        
        // Check that all dependencies are already present in pipe
        for &dep in &deps {
            if self.get_layer_by_name(dep).is_none() {
                return Err(DependencyError::MissingDependency {
                    middleware: mw_name.to_string(),
                    dependency: dep.to_string(),
                });
            }
        }
        
        // Clone the inner Vec and append the boxed middleware.
        let mut new_vec = (*self.inner).clone();
        new_vec.push(mw);
        Ok(Self {
            inner: Arc::new(new_vec),
        })
    }
    
    /// Validate the entire pipe for correct dependency order
    pub fn validate_order(&self) -> Result<(), Vec<DependencyError>> {
        let mut errors = Vec::new();
        
        for (pos, mw) in self.inner.iter().enumerate() {
            let mw_name = mw.name().unwrap_or("unnamed");
            
            for &dep in &mw.dependencies() {
                // Search for dependency in middleware that come before current position
                let dep_found = self.inner.iter().take(pos)
                    .any(|m| m.name().map_or(false, |name| name == dep));
                    
                if !dep_found {
                    errors.push(DependencyError::WrongOrder {
                        middleware: mw_name.to_string(),
                        dependency: dep.to_string(),
                    });
                }
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
    
    /// Execute middleware with dependency validation
    pub async fn execute_with_validation<S>(
        &self,
        inner: S,
        ctx: Context<()>,
        req: Request<Body>,
    ) -> Result<Response<Body>, BoxError>
    where
        S: Service<(), Request<Body>, Response = Response<Body>> + Clone + Send + Sync + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        // Validate order before execution
        if let Err(errors) = self.validate_order() {
            let error_msg = errors.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(format!("Dependency order errors: {}", error_msg).into());
        }
        
        self.execute_middleware(inner, ctx, req).await
    }
    /// Execute the middleware chain for a concrete inner service `S`.
    ///
    /// This converts the concrete `inner` service to a boxed type (`BoxService`) once,
    /// borrows a slice from the internal Arc<Vec<...>> and runs the optimized `Next`.
    /// Hot path: no atomic ops per middleware.
    pub async fn execute_middleware<S>(
        &self,
        inner: S,
        ctx: Context<()>,
        req: Request<Body>,
    ) -> Result<Response<Body>, BoxError>
    where
        S: Service<(), Request<Body>, Response = Response<Body>> + Clone + Send + Sync + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let slice: &[BoxedMiddleware] = &*self.inner;

        // Convert the concrete service to BoxService
        let boxed_service = crate::httpward_middleware::adapter::box_service_from(inner);

        let next = Next::new(slice, &boxed_service);

        next.run(ctx, req).await
    }
}


/// C-compatible representation of a fat pointer to dyn HttpWardMiddleware.
#[repr(C)]
pub struct MiddlewareFatPtr {
    pub data: *mut c_void,
    pub vtable: *mut c_void,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rama::http::{Body, Request, Response};
    use rama::Context;
    use crate::httpward_middleware::types::BoxError;
    use crate::httpward_middleware::next::Next;

    // Minimal test middleware to verify plumbing.
    struct DummyMw;
    #[async_trait]
    impl HttpWardMiddleware for DummyMw {
        async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
            // call next without changes
            next.run(ctx, req).await
        }
        
        fn name(&self) -> Option<&'static str> {
            Some("DummyMw")
        }
    }

    // Test middleware with dependencies
    struct DependentMw;
    #[async_trait]
    impl HttpWardMiddleware for DependentMw {
        async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
            next.run(ctx, req).await
        }
        
        fn name(&self) -> Option<&'static str> {
            Some("DependentMw")
        }
        
        fn dependencies(&self) -> Vec<&'static str> {
            vec!["DummyMw"]
        }
    }

    // Test middleware with name but no dependencies
    struct NamedMw;
    #[async_trait]
    impl HttpWardMiddleware for NamedMw {
        async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
            next.run(ctx, req).await
        }
        
        fn name(&self) -> Option<&'static str> {
            Some("NamedMw")
        }
    }

    
    #[test]
    fn test_add_layer_success() {
        let pipe = HttpWardMiddlewarePipe::new()
            .add_layer(DummyMw)
            .unwrap()
            .add_layer(DependentMw)
            .unwrap();
            
        assert_eq!(pipe.len(), 2);
        assert!(pipe.get_layer_by_name("DummyMw").is_some());
        assert!(pipe.get_layer_by_name("DependentMw").is_some());
    }
    
    #[test]
    fn test_add_layer_missing_dependency() {
        let pipe = HttpWardMiddlewarePipe::new();
        let result = pipe.add_layer(DependentMw);
        
        assert!(result.is_err());
        match result.unwrap_err() {
            DependencyError::MissingDependency { middleware, dependency } => {
                assert_eq!(middleware, "DependentMw");
                assert_eq!(dependency, "DummyMw");
            }
            _ => panic!("Expected MissingDependency error"),
        }
    }
    
    #[test]
    fn test_validate_order_success() {
        let pipe = HttpWardMiddlewarePipe::new()
            .add_layer(DummyMw)
            .unwrap()
            .add_layer(DependentMw)
            .unwrap();
            
        assert!(pipe.validate_order().is_ok());
    }
    
    #[test]
    fn test_validate_order_wrong_order() {
        // Create pipe with wrong order: add both middleware, then manually reorder for testing
        let pipe = HttpWardMiddlewarePipe::new()
            .add_layer(DummyMw)
            .unwrap()
            .add_layer(DependentMw)
            .unwrap();
            
        // This pipe should have correct order, so validate_order should pass
        assert!(pipe.validate_order().is_ok());
        
        // For testing wrong order, we need to create a scenario where dependencies exist but are in wrong position
        // Since add_layer enforces dependencies, we can't create a truly wrong order at build time
        // But we can test the validation logic by creating a pipe manually with wrong order
        let mut wrong_order_vec: Vec<BoxedMiddleware> = Vec::new();
        wrong_order_vec.push(Arc::new(DependentMw) as BoxedMiddleware);  // Add dependent first
        wrong_order_vec.push(Arc::new(DummyMw) as BoxedMiddleware);      // Add dependency second
        
        let wrong_order_pipe = HttpWardMiddlewarePipe {
            inner: Arc::new(wrong_order_vec),
        };
        let result = wrong_order_pipe.validate_order();
        assert!(result.is_err());
        
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        match &errors[0] {
            DependencyError::WrongOrder { middleware, dependency } => {
                assert_eq!(middleware, "DependentMw");
                assert_eq!(dependency, "DummyMw");
            }
            _ => panic!("Expected WrongOrder error"),
        }
    }
    
    #[test]
    fn test_get_layer_by_name() {
        let pipe = HttpWardMiddlewarePipe::new()
            .add_layer(NamedMw)
            .unwrap()
            .add_layer(DummyMw)
            .unwrap();
            
        assert!(pipe.get_layer_by_name("NamedMw").is_some());
        assert!(pipe.get_layer_by_name("DummyMw").is_some()); // DummyMw now has a name
        assert!(pipe.get_layer_by_name("NonExistent").is_none());
    }
}
