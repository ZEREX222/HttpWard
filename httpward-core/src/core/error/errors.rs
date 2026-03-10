// File: httpward-core/src/httpward_middleware/errors.rs

use std::fmt;
use rama::http::StatusCode;

/// Custom error types for HttpWard middleware
#[derive(Debug, Clone)]
pub enum HttpWardMiddlewareError {
    /// Authentication failed
    AuthenticationFailed {
        code: u16,
        title: String,
        description: String,
    },
    /// Authorization failed  
    AuthorizationFailed {
        code: u16,
        title: String,
        description: String,
    },
    /// Request validation failed
    ValidationError {
        code: u16,
        title: String,
        description: String,
    },
    /// Rate limit exceeded
    RateLimitExceeded {
        code: u16,
        title: String,
        description: String,
    },
    /// Custom business logic error
    BusinessLogicError {
        code: u16,
        title: String,
        description: String,
    },
    /// Generic error with custom parameters
    Custom {
        code: u16,
        title: String,
        description: String,
    },
}

impl HttpWardMiddlewareError {
    /// Create authentication failed error
    pub fn auth_failed(message: &str) -> Self {
        Self::AuthenticationFailed {
            code: 401,
            title: "Authentication Failed".to_string(),
            description: message.to_string(),
        }
    }

    /// Create authorization failed error
    pub fn authz_failed(message: &str) -> Self {
        Self::AuthorizationFailed {
            code: 403,
            title: "Access Forbidden".to_string(),
            description: message.to_string(),
        }
    }

    /// Create validation error
    pub fn validation_failed(message: &str) -> Self {
        Self::ValidationError {
            code: 400,
            title: "Validation Error".to_string(),
            description: message.to_string(),
        }
    }

    /// Create rate limit error
    pub fn rate_limit_exceeded(message: &str) -> Self {
        Self::RateLimitExceeded {
            code: 429,
            title: "Rate Limit Exceeded".to_string(),
            description: message.to_string(),
        }
    }

    /// Create business logic error
    pub fn business_error(code: u16, title: &str, description: &str) -> Self {
        Self::BusinessLogicError {
            code,
            title: title.to_string(),
            description: description.to_string(),
        }
    }

    /// Create custom error
    pub fn custom(code: u16, title: &str, description: &str) -> Self {
        Self::Custom {
            code,
            title: title.to_string(),
            description: description.to_string(),
        }
    }

    /// Get HTTP status code for this error
    pub fn status_code(&self) -> StatusCode {
        let code = match self {
            Self::AuthenticationFailed { code, .. } => *code,
            Self::AuthorizationFailed { code, .. } => *code,
            Self::ValidationError { code, .. } => *code,
            Self::RateLimitExceeded { code, .. } => *code,
            Self::BusinessLogicError { code, .. } => *code,
            Self::Custom { code, .. } => *code,
        };
        
        StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Get error title
    pub fn title(&self) -> &str {
        match self {
            Self::AuthenticationFailed { title, .. } => title,
            Self::AuthorizationFailed { title, .. } => title,
            Self::ValidationError { title, .. } => title,
            Self::RateLimitExceeded { title, .. } => title,
            Self::BusinessLogicError { title, .. } => title,
            Self::Custom { title, .. } => title,
        }
    }

    /// Get error description
    pub fn description(&self) -> &str {
        match self {
            Self::AuthenticationFailed { description, .. } => description,
            Self::AuthorizationFailed { description, .. } => description,
            Self::ValidationError { description, .. } => description,
            Self::RateLimitExceeded { description, .. } => description,
            Self::BusinessLogicError { description, .. } => description,
            Self::Custom { description, .. } => description,
        }
    }
}

impl fmt::Display for HttpWardMiddlewareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.status_code().as_u16(), self.title(), self.description())
    }
}

impl std::error::Error for HttpWardMiddlewareError {}

/// Convenient constructors for common error types
pub struct HttpWardError;

impl HttpWardError {
    pub fn auth_failed(message: &str) -> HttpWardMiddlewareError {
        HttpWardMiddlewareError::auth_failed(message)
    }

    pub fn authz_failed(message: &str) -> HttpWardMiddlewareError {
        HttpWardMiddlewareError::authz_failed(message)
    }

    pub fn validation_failed(message: &str) -> HttpWardMiddlewareError {
        HttpWardMiddlewareError::validation_failed(message)
    }

    pub fn rate_limit_exceeded(message: &str) -> HttpWardMiddlewareError {
        HttpWardMiddlewareError::rate_limit_exceeded(message)
    }

    pub fn business_error(code: u16, title: &str, description: &str) -> HttpWardMiddlewareError {
        HttpWardMiddlewareError::business_error(code, title, description)
    }

    pub fn custom(code: u16, title: &str, description: &str) -> HttpWardMiddlewareError {
        HttpWardMiddlewareError::custom(code, title, description)
    }
}

/// Trait to check if an error is a HttpWardMiddlewareError
pub trait IsHttpWardError {
    fn as_httpward_error(&self) -> Option<&HttpWardMiddlewareError>;
}

impl IsHttpWardError for Box<dyn std::error::Error + Send + Sync> {
    fn as_httpward_error(&self) -> Option<&HttpWardMiddlewareError> {
        self.downcast_ref::<HttpWardMiddlewareError>()
    }
}
