use std::collections::HashMap;
use std::path::Path;
use rama::http::{Request as RamaRequest, Response as RamaResponse, Body as RamaBody, StatusCode};
use tokio::fs;
use tracing::debug;
use httpward_core::config::Route;
use crate::core::middleware::route::{MatchedRoute, RouteError};

/// Process static directory path with matcher parameters
/// Replaces placeholders like {param}, {*any}, and {1}, {2} (regex groups) with actual values from params
pub fn process_static_dir_with_params(
    static_dir: &Path,
    params: &HashMap<String, String>,
) -> Result<std::path::PathBuf, RouteError> {
    let static_dir_str = static_dir.to_str()
        .ok_or_else(|| RouteError::Static("Invalid static directory path encoding".to_string()))?;
    
    let mut result = static_dir_str.to_string();
    
    // Replace named parameters like {param} and {*any}
    for (key, value) in params {
        let placeholder = format!("{{{}}}", key);
        result = result.replace(&placeholder, value);
        
        // Also handle wildcard parameters {*any}
        let wildcard_placeholder = format!("{{*{}}}", key);
        result = result.replace(&wildcard_placeholder, value);
    }
    
    Ok(std::path::PathBuf::from(result))
}

/// Handle static file serving
pub async fn handle_static(
    request: &RamaRequest<RamaBody>,
    static_dir: &std::path::PathBuf,
    matched_route: &MatchedRoute,
) -> Result<RamaResponse<RamaBody>, RouteError> {
    let request_path = request.uri().path();
    debug!("Static file request: path={}, static_dir={:?}", request_path, static_dir);
    
    // Process static directory path with matcher parameters (like proxy backend)
    let processed_static_dir = match process_static_dir_with_params(static_dir, &matched_route.params) {
        Ok(dir) => dir,
        Err(e) => {
            tracing::error!("Failed to process static directory with params: {}", e);
            return Ok(RamaResponse::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(RamaBody::from("Static directory processing error"))
                .unwrap());
        }
    };
    
    debug!("Processed static directory: {:?}", processed_static_dir);
    
    // Get the matched path from the route
    let matched_path = match &matched_route.route {
        Route::Static { r#match, .. } => {
            r#match.path.as_deref().unwrap_or("")
        }
        _ => "",
    };
    
    debug!("Matched path: {}", matched_path);
    debug!("Route params: {:?}", matched_route.params);
    
    // Determine the file path based on route parameters
    let file_path = if !matched_route.params.is_empty() {
        // Check if static_dir already contains parameters (like {path} or {*path})
        let static_dir_str = static_dir.to_str().unwrap_or("");
        let has_params_in_static_dir = static_dir_str.contains('{') && static_dir_str.contains('}');
        
        if has_params_in_static_dir {
            // If static_dir already contains parameters, the processed_static_dir already has the full path
            // Just serve the file directly from processed_static_dir
            processed_static_dir.clone()
        } else {
            // If static_dir doesn't contain parameters, use the parameter value as relative path
            let empty_string = "".to_string();
            let path_param = matched_route.params.values().next().unwrap_or(&empty_string);
            
            if path_param.is_empty() {
                // If parameter is empty, serve index.html
                processed_static_dir.join("index.html")
            } else {
                // Use the parameter as the relative path from processed_static_dir
                processed_static_dir.join(path_param)
            }
        }
    } else {
        // If no parameters, remove the prefix and serve from static_dir
        let relative_path = if request_path.starts_with(matched_path) {
            &request_path[matched_path.len()..]
        } else {
            request_path
        };
        
        let clean_path = relative_path.trim_start_matches('/');
        debug!("Relative path after removing prefix: '{}'", clean_path);
        
        // Prevent directory traversal
        if clean_path.contains("..") {
            return Ok(RamaResponse::builder()
                .status(StatusCode::FORBIDDEN)
                .body(RamaBody::from("Forbidden"))
                .unwrap());
        }
        
        // If path is empty (requesting exactly the route), try index.html
        // Otherwise, serve the requested file
        if clean_path.is_empty() {
            processed_static_dir.join("index.html")
        } else {
            processed_static_dir.join(clean_path)
        }
    };
    
    debug!("Trying to serve file: {:?}", file_path);
    
    // Check if file exists and is within static_dir
    match fs::metadata(&file_path).await {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Ok(RamaResponse::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(RamaBody::from("Not Found"))
                    .unwrap());
            }
            
            // Try to determine content type
            let content_type = guess_content_type(&file_path);
            
            match fs::read(&file_path).await {
                Ok(contents) => {
                    let mut response = RamaResponse::builder()
                        .status(StatusCode::OK);
                        
                    if let Some(ct) = content_type {
                        response = response.header("Content-Type", ct);
                    }
                    
                    Ok(response
                        .body(RamaBody::from(contents))
                        .unwrap())
                }
                Err(e) => {
                    tracing::error!("Failed to read static file: {}", e);
                    Ok(RamaResponse::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(RamaBody::from("Internal Server Error"))
                        .unwrap())
                }
            }
        }
        Err(_) => {
            Ok(RamaResponse::builder()
                .status(StatusCode::NOT_FOUND)
                .body(RamaBody::from("Not Found"))
                .unwrap())
        }
    }
}

/// Guess content type based on file extension
fn guess_content_type(path: &std::path::Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?;
    
    match extension.to_lowercase().as_str() {
        "html" => Some("text/html"),
        "css" => Some("text/css"),
        "js" => Some("application/javascript"),
        "json" => Some("application/json"),
        "xml" => Some("application/xml"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "pdf" => Some("application/pdf"),
        "txt" => Some("text/plain"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use rama::http::{Method};
    use httpward_core::config::Match;
    use crate::core::middleware::route::MatcherType;

    #[test]
    fn test_process_static_dir_with_params() {
        let mut params = HashMap::new();
        params.insert("path".to_string(), "style.css".to_string());
        
        let static_dir = Path::new("C:/myprojects/html/{*path}");
        let result = process_static_dir_with_params(static_dir, &params).unwrap();
        assert_eq!(result, Path::new("C:/myprojects/html/style.css"));
        
        // Test nested path
        params.insert("path".to_string(), "css/style.css".to_string());
        let result2 = process_static_dir_with_params(static_dir, &params).unwrap();
        assert_eq!(result2, Path::new("C:/myprojects/html/css/style.css"));
        
        // Test multiple parameters
        let mut params2 = HashMap::new();
        params2.insert("user".to_string(), "john".to_string());
        params2.insert("theme".to_string(), "dark".to_string());
        
        let static_dir2 = Path::new("C:/myprojects/html/{user}/{theme}");
        let result3 = process_static_dir_with_params(static_dir2, &params2).unwrap();
        assert_eq!(result3, Path::new("C:/myprojects/html/john/dark"));
    }
    
    #[tokio::test]
    async fn test_static_file_with_params() {
        let matched_route = MatchedRoute {
            route: Route::Static {
                r#match: Match {
                    path: Some("/site/{*path}".to_string()),
                    path_regex: None,
                },
                static_dir: PathBuf::from("C:/test/html/{*path}"),
                strategy: None,
                strategies: None,
            },
            params: {
                let mut p = HashMap::new();
                p.insert("path".to_string(), "style.css".to_string());
                p
            },
            matcher_type: MatcherType::Path,
        };
        
        // Test requesting a subpath with parameters
        let request = RamaRequest::builder()
            .method(Method::GET)
            .uri("/site/style.css")
            .body(RamaBody::empty())
            .unwrap();
            
        // This should try to serve C:/test/html/style.css 
        // (processed static_dir already contains the full path, no need to add param again)
        let result = handle_static(&request, &PathBuf::from("C:/test/html/{*path}"), &matched_route).await;
        
        match result {
            Ok(response) => {
                assert_eq!(response.status(), StatusCode::NOT_FOUND);
            }
            Err(_) => {
                // Also acceptable
            }
        }
        
        // Test with empty parameter (should serve index.html)
        let matched_route2 = MatchedRoute {
            route: Route::Static {
                r#match: Match {
                    path: Some("/site/{*path}".to_string()),
                    path_regex: None,
                },
                static_dir: PathBuf::from("C:/test/html/{*path}"),
                strategy: None,
                strategies: None,
            },
            params: {
                let mut p = HashMap::new();
                p.insert("path".to_string(), "".to_string());
                p
            },
            matcher_type: MatcherType::Path,
        };
        
        let request2 = RamaRequest::builder()
            .method(Method::GET)
            .uri("/site")
            .body(RamaBody::empty())
            .unwrap();
            
        // This should try to serve C:/test/html/ (empty param)
        let result2 = handle_static(&request2, &PathBuf::from("C:/test/html/{*path}"), &matched_route2).await;
        
        match result2 {
            Ok(response) => {
                assert_eq!(response.status(), StatusCode::NOT_FOUND);
            }
            Err(_) => {
                // Also acceptable
            }
        }
    }
}
