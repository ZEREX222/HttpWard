#[cfg(test)]
mod tests {
    use crate::httpward_middleware::HttpWardMiddlewarePipe;
    use crate::httpward_middleware::layers::log::HttpWardLogLayer;

    #[test]
    fn test_empty_pipe() {
        let pipe = HttpWardMiddlewarePipe::new();
        assert_eq!(pipe.len(), 0);
        assert!(pipe.is_empty());
    }

    #[test]
    fn test_layer_by_name() {
        let pipe = HttpWardMiddlewarePipe::new()
            .add_layer(HttpWardLogLayer::new().with_tag("test"));

        let layer = pipe.get_layer_by_name("HttpWardLogLayer");
        assert!(layer.is_some());
    }

    #[test]
    fn test_add_layer() {
        let pipe = HttpWardMiddlewarePipe::new()
            .add_layer(HttpWardLogLayer::new());
        
        assert_eq!(pipe.len(), 1);
    }
}
