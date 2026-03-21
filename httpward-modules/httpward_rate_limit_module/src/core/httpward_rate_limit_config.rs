use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for HttpWard Rate Limit Module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpWardRateLimitConfig {
    /// Session timeout duration
    pub session_timeout: Option<Duration>,

    /// Cookie configuration
    pub cookie_config: Option<CookieConfig>,

    /// Authentication configuration
    pub auth_config: Option<AuthConfig>,

    /// Session storage configuration
    pub storage_config: Option<StorageConfig>,
}

impl Default for HttpWardRateLimitConfig {
    fn default() -> Self {
        Self {
            session_timeout: Some(Duration::from_secs(3600)), // 1 hour default
            cookie_config: Some(CookieConfig::default()),
            auth_config: Some(AuthConfig::default()),
            storage_config: Some(StorageConfig::default()),
        }
    }
}

impl HttpWardRateLimitConfig {
    /// Create new configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set session timeout
    pub fn with_session_timeout(mut self, timeout: Duration) -> Self {
        self.session_timeout = Some(timeout);
        self
    }

    /// Set cookie configuration
    pub fn with_cookie_config(mut self, config: CookieConfig) -> Self {
        self.cookie_config = Some(config);
        self
    }

    /// Set authentication configuration
    pub fn with_auth_config(mut self, config: AuthConfig) -> Self {
        self.auth_config = Some(config);
        self
    }

    /// Set storage configuration
    pub fn with_storage_config(mut self, config: StorageConfig) -> Self {
        self.storage_config = Some(config);
        self
    }
}

/// Cookie configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieConfig {
    /// Cookie name
    pub name: String,

    /// Cookie domain
    pub domain: Option<String>,

    /// Cookie path
    pub path: Option<String>,

    /// Secure flag
    pub secure: bool,

    /// HttpOnly flag
    pub http_only: bool,

    /// SameSite policy
    pub same_site: Option<String>,
}

impl Default for CookieConfig {
    fn default() -> Self {
        Self {
            name: "httpward_session".to_string(),
            domain: None,
            path: Some("/".to_string()),
            secure: true,
            http_only: true,
            same_site: Some("Lax".to_string()),
        }
    }
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Authentication type
    pub auth_type: AuthType,

    /// JWT configuration (if using JWT)
    pub jwt_config: Option<JwtConfig>,

    /// Basic auth configuration (if using Basic auth)
    pub basic_config: Option<BasicAuthConfig>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            auth_type: AuthType::Session,
            jwt_config: None,
            basic_config: None,
        }
    }
}

/// Authentication types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    Session,
    Jwt,
    Basic,
    Custom,
}

/// JWT configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    /// Secret key
    pub secret: String,

    /// Algorithm
    pub algorithm: Option<String>,

    /// Expiration
    pub expiration: Option<Duration>,
}

/// Basic authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicAuthConfig {
    /// Realm
    pub realm: Option<String>,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Storage type
    pub storage_type: StorageType,

    /// Redis configuration (if using Redis)
    pub redis_config: Option<RedisConfig>,

    /// Memory storage configuration
    pub memory_config: Option<MemoryConfig>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            storage_type: StorageType::Memory,
            redis_config: None,
            memory_config: Some(MemoryConfig::default()),
        }
    }
}

/// Storage types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageType {
    Memory,
    Redis,
    Database,
    Custom,
}

/// Redis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    /// Redis URL
    pub url: String,

    /// Key prefix
    pub key_prefix: Option<String>,
}

/// Memory storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum number of sessions
    pub max_sessions: Option<usize>,

    /// Cleanup interval
    pub cleanup_interval: Option<Duration>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_sessions: Some(10000),
            cleanup_interval: Some(Duration::from_secs(300)), // 5 minutes
        }
    }
}

