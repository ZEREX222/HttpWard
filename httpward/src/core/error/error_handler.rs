use rama::http::{Response, Body, StatusCode};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct ErrorHandler {
    template_content: String,
}

impl ErrorHandler {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let template_path = Path::new("httpward/assets/error.html");
        let template_content = fs::read_to_string(template_path)
            .map_err(|e| format!("Failed to read error template: {}", e))?;
        
        Ok(Self { template_content })
    }

    pub fn create_error_response(&self, status: StatusCode, title: &str, description: &str) -> Result<Response<Body>, Box<dyn std::error::Error>> {
        let status_code = status.as_u16();
        let content = self.template_content
            .replace("{{e_num}}", &status_code.to_string())
            .replace("{{e_text}}", title)
            .replace("{{e_desc}}", description);

        Ok(Response::builder()
            .status(status)
            .header("content-type", "text/html; charset=utf-8")
            .body(Body::from(content))
            .unwrap())
    }

    pub fn create_error_response_with_code(&self, status: StatusCode) -> Result<Response<Body>, Box<dyn std::error::Error>> {
        let (title, description) = match status {
            StatusCode::NOT_FOUND => (
                "Page Not Found",
                "The requested page could not be found on this server."
            ),
            StatusCode::INTERNAL_SERVER_ERROR => (
                "Internal Server Error",
                "An unexpected error occurred while processing your request."
            ),
            StatusCode::BAD_GATEWAY => (
                "Bad Gateway",
                "The server encountered an error while trying to proxy your request."
            ),
            StatusCode::FORBIDDEN => (
                "Access Forbidden",
                "You do not have permission to access this resource."
            ),
            StatusCode::UNAUTHORIZED => (
                "Unauthorized",
                "Authentication is required to access this resource."
            ),
            StatusCode::BAD_REQUEST => (
                "Bad Request",
                "The server cannot process your request due to invalid syntax."
            ),
            _ => (
                "Error",
                "An error occurred while processing your request."
            ),
        };

        self.create_error_response(status, title, description)
    }
}

impl Default for ErrorHandler {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            template_content: include_str!("../../../assets/error.html").to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rama::http::StatusCode;

    #[test]
    fn test_error_handler_creation() {
        let handler = ErrorHandler::default();
        assert!(!handler.template_content.is_empty());
        assert!(handler.template_content.contains("{{e_num}}"));
        assert!(handler.template_content.contains("{{e_text}}"));
        assert!(handler.template_content.contains("{{e_desc}}"));
    }

    #[test]
    fn test_404_error_response() {
        let handler = ErrorHandler::default();
        let response = handler.create_error_response_with_code(StatusCode::NOT_FOUND).unwrap();
        
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers().get("content-type").unwrap(), "text/html; charset=utf-8");
    }

    #[test]
    fn test_500_error_response() {
        let handler = ErrorHandler::default();
        let response = handler.create_error_response_with_code(StatusCode::INTERNAL_SERVER_ERROR).unwrap();
        
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_custom_error_response() {
        let handler = ErrorHandler::default();
        let response = handler.create_error_response(
            StatusCode::BAD_REQUEST, 
            "Custom Error", 
            "This is a custom error description"
        ).unwrap();
        
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_template_replacement() {
        let handler = ErrorHandler::default();
        let template = handler.template_content.clone();
        
        // Test that template contains placeholders
        assert!(template.contains("{{e_num}}"));
        assert!(template.contains("{{e_text}}"));
        assert!(template.contains("{{e_desc}}"));
        
        // Test replacement logic
        let result = template
            .replace("{{e_num}}", "404")
            .replace("{{e_text}}", "Not Found")
            .replace("{{e_desc}}", "Page not found");
        
        assert!(result.contains("404"));
        assert!(result.contains("Not Found"));
        assert!(result.contains("Page not found"));
        assert!(!result.contains("{{e_num}}"));
        assert!(!result.contains("{{e_text}}"));
        assert!(!result.contains("{{e_desc}}"));
    }
}
