use crate::middleware::core::ContentType;

/// Parse content type string into ContentType enum
pub fn parse_content_type(content_type_str: &str) -> ContentType {
    let content_type_str = content_type_str.to_lowercase();
    
    if content_type_str.contains("text/html") {
        ContentType::Html
    } else if content_type_str.contains("application/json") {
        ContentType::Json
    } else if content_type_str.contains("application/xml") || content_type_str.contains("text/xml") {
        ContentType::Xml
    } else if content_type_str.contains("text/plain") {
        ContentType::PlainText
    } else if content_type_str.contains("text/css") {
        ContentType::Css
    } else if content_type_str.contains("application/javascript") || content_type_str.contains("text/javascript") {
        ContentType::JavaScript
    } else if content_type_str.contains("image/") {
        ContentType::Image
    } else if content_type_str.contains("video/") {
        ContentType::Video
    } else if content_type_str.contains("application/pdf") {
        ContentType::Pdf
    } else if content_type_str.contains("application/grpc") {
        ContentType::Grpc
    } else if content_type_str.contains("application/x-www-form-urlencoded") {
        ContentType::FormUrlEncoded
    } else if content_type_str.contains("multipart/form-data") {
        ContentType::Multipart
    } else if content_type_str.contains("application/octet-stream") {
        ContentType::OctetStream
    } else if content_type_str.contains("text/event-stream") {
        ContentType::EventStream
    } else if content_type_str.contains("font/") {
        ContentType::Font
    } else {
        ContentType::Unknown
    }
}
