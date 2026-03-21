#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::httpward_middleware::HttpWardMiddlewarePipe;
    use crate::httpward_middleware::middleware_trait::HttpWardMiddleware;
    use crate::httpward_middleware::next::Next;
    use crate::httpward_middleware::types::BoxError;
    use async_trait::async_trait;
    use rama::{
        Context,
        http::{Body, Request, Response},
    };
    use std::fmt::Debug;

    // Simple test middleware for testing purposes
    #[derive(Clone, Debug)]
    struct TestMiddleware;

    #[async_trait]
    impl HttpWardMiddleware for TestMiddleware {
        async fn handle(
            &self,
            _ctx: Context<()>,
            _req: Request<Body>,
            next: Next<'_>,
        ) -> Result<Response<Body>, BoxError> {
            next.run(_ctx, _req).await
        }

        fn name(&self) -> Option<&'static str> {
            Some("TestMiddleware")
        }
    }

    #[test]
    fn test_empty_pipe() {
        let pipe = HttpWardMiddlewarePipe::new();
        assert_eq!(pipe.len(), 0);
        assert!(pipe.is_empty());
    }

    #[test]
    fn test_layer_by_name() {
        let pipe = HttpWardMiddlewarePipe::new()
            .add_layer(TestMiddleware)
            .unwrap();

        let layer = pipe.get_layer_by_name("TestMiddleware");
        assert!(layer.is_some());
    }

    #[test]
    fn test_add_layer() {
        let pipe = HttpWardMiddlewarePipe::new()
            .add_layer(TestMiddleware)
            .unwrap();

        assert_eq!(pipe.len(), 1);
    }
}
