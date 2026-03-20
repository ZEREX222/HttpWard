#[cfg(test)]
mod tests {
    use crate::middleware_config_ext::get_config_from_middleware;
    use crate::httpward_middleware::middleware_trait::HttpWardMiddleware;
    use crate::httpward_middleware::next::Next;
    use crate::httpward_middleware::types::BoxError;
    use rama::{Context, http::{Request, Body}};
    use serde::Deserialize;
    use async_trait::async_trait;
    
    #[derive(Debug, Deserialize)]
    struct TestConfig {
        enabled: bool,
        level: String,
    }
    
    struct TestMiddleware {
        name: &'static str,
    }
    
    #[async_trait]
    impl HttpWardMiddleware for TestMiddleware {
        async fn handle(
            &self,
            _ctx: Context<()>,
            _req: Request<Body>,
            _next: Next<'_>,
        ) -> Result<rama::http::Response<Body>, BoxError> {
            Ok(rama::http::Response::new(Body::empty()))
        }
        
        fn name(&self) -> Option<&'static str> {
            Some(self.name)
        }
    }
    
    #[test]
    fn test_get_config_from_middleware_uses_middleware_name() {
        let middleware = TestMiddleware { name: "TestMiddleware" };
        
        // Create a mock context and request
        let ctx = Context::default();
        let req = Request::builder()
            .uri("http://example.com/test")
            .body(Body::empty())
            .unwrap();
        
        // This should try to get config using "TestMiddleware" as the name
        let result = get_config_from_middleware::<TestConfig, _>(&ctx, &req, &middleware);
        
        // We expect this to fail because no HttpWardContext is set up,
        // but the error should contain the middleware name
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        println!("Error message: {}", error_msg);
        // The error should be about HttpWardContext not found, not about middleware name
        assert!(error_msg.contains("HttpWardContext not found"));
    }
    
    #[test]
    fn test_middleware_without_name_returns_error() {
        struct NamelessMiddleware;
        
        #[async_trait]
        impl HttpWardMiddleware for NamelessMiddleware {
            async fn handle(
                &self,
                _ctx: Context<()>,
                _req: Request<Body>,
                _next: Next<'_>,
            ) -> Result<rama::http::Response<Body>, BoxError> {
                Ok(rama::http::Response::new(Body::empty()))
            }
            
            // No name() implementation - returns None
        }
        
        let middleware = NamelessMiddleware;
        let ctx = Context::default();
        let req = Request::builder()
            .uri("http://example.com/test")
            .body(Body::empty())
            .unwrap();
        
        let result = get_config_from_middleware::<TestConfig, _>(&ctx, &req, &middleware);
        
        // Should fail because middleware has no name
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("does not have a name"));
    }
    
    #[test]
    fn test_middleware_name_extraction() {
        let middleware = TestMiddleware { name: "CustomMiddlewareName" };
        
        // Test that name() returns the expected value
        assert_eq!(middleware.name(), Some("CustomMiddlewareName"));
        
        // Test with different name
        let middleware2 = TestMiddleware { name: "AnotherName" };
        assert_eq!(middleware2.name(), Some("AnotherName"));
    }
}
