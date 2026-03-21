use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Context for rate limit management
/// This will be stored in HttpWardContext.extensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpWardRateLimitContext {
    /// User identity information
    pub user_id: Option<String>,
    /// Session data
    pub session_data: HashMap<String, String>,
    /// Authentication status
    pub is_authenticated: bool,
    /// Session expiration
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Header fingerprint for client identification
    pub header_fp: Option<String>,
    /// JA4 fingerprint for TLS client identification
    pub ja4_fp: Option<String>,
}

impl Default for HttpWardRateLimitContext {
    fn default() -> Self {
        Self {
            user_id: None,
            session_data: HashMap::new(),
            is_authenticated: false,
            expires_at: None,
            header_fp: None,
            ja4_fp: None,
        }
    }
}

impl HttpWardRateLimitContext {
    /// Create new rate limit context
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

    /// Set header fingerprint
    pub fn with_header_fp(mut self, header_fp: String) -> Self {
        self.header_fp = Some(header_fp);
        self
    }

    /// Set JA4 fingerprint
    pub fn with_ja4_fp(mut self, ja4_fp: String) -> Self {
        self.ja4_fp = Some(ja4_fp);
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
        self.header_fp = None;
        self.ja4_fp = None;
    }
}

