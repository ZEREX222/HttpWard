// File: httpward-core/src/httpward_middleware/dependency_error.rs

use std::fmt;

/// Errors related to middleware dependency validation
#[derive(Debug, Clone)]
pub enum DependencyError {
    /// Middleware requires a dependency that is not present in pipe
    MissingDependency { 
        middleware: String, 
        dependency: String 
    },
    
    /// Middleware depends on another middleware that comes after it in pipe
    WrongOrder { 
        middleware: String, 
        dependency: String 
    },
}

impl fmt::Display for DependencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DependencyError::MissingDependency { middleware, dependency } => {
                write!(f, "Middleware '{}' requires dependency '{}' which is not in pipe (check the order!)", middleware, dependency)
            }
            DependencyError::WrongOrder { middleware, dependency } => {
                write!(f, "Middleware '{}' depends on '{}' which comes after it in pipe", middleware, dependency)
            }
        }
    }
}

impl std::error::Error for DependencyError {}
