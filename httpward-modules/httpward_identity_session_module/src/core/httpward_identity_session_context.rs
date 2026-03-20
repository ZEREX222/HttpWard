use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Context for identity and session management
/// This will be stored in HttpWardContext.extensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpWardIdentitySessionContext {
    /// User identity information
    pub user_id: Option<String>,
    /// Session data
    pub session_data: HashMap<String, String>,
    /// Authentication status
    pub is_authenticated: bool,
    /// Session expiration
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for HttpWardIdentitySessionContext {
    fn default() -> Self {
        Self {
            user_id: None,
            session_data: HashMap::new(),
            is_authenticated: false,
            expires_at: None,
        }
    }
}

impl HttpWardIdentitySessionContext {
    /// Create new identity session context
    pub fn new() -> Self {
        Self::default()
    }

    /// Set user identity
    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self.is_authenticated = true;
        self
    }

    /// Add session data
    pub fn with_session_data(mut self, key: String, value: String) -> Self {
        self.session_data.insert(key, value);
        self
    }

    /// Set session expiration
    pub fn with_expiration(mut self, expires_at: chrono::DateTime<chrono::Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Get session data value
    pub fn get_session_data(&self, key: &str) -> Option<&String> {
        self.session_data.get(key)
    }

    /// Check if session is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            chrono::Utc::now() > expires_at
        } else {
            false
        }
    }

    /// Clear session data
    pub fn clear(&mut self) {
        self.user_id = None;
        self.session_data.clear();
        self.is_authenticated = false;
        self.expires_at = None;
    }
}
