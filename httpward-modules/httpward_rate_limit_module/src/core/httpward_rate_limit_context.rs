use serde::{Deserialize, Serialize};

/// Request-scoped data produced by the rate limit middleware.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HttpWardRateLimitContext {
    /// Client IP used for IP based limiting.
    pub client_ip: Option<String>,
    /// Route scope derived from `HttpWardContext.matched_route`.
    pub matched_route_scope: Option<String>,
    /// Header fingerprint for client identification.
    pub header_fp: Option<String>,
    /// JA4 fingerprint for TLS client identification.
    pub ja4_fp: Option<String>,
}

impl HttpWardRateLimitContext {
    /// Create new rate limit context.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_client_ip(mut self, client_ip: String) -> Self {
        self.client_ip = Some(client_ip);
        self
    }

    pub fn with_matched_route_scope(mut self, matched_route_scope: String) -> Self {
        self.matched_route_scope = Some(matched_route_scope);
        self
    }

    pub fn with_header_fp(mut self, header_fp: String) -> Self {
        self.header_fp = Some(header_fp);
        self
    }

    pub fn with_ja4_fp(mut self, ja4_fp: String) -> Self {
        self.ja4_fp = Some(ja4_fp);
        self
    }

    pub fn clear(&mut self) {
        self.client_ip = None;
        self.matched_route_scope = None;
        self.header_fp = None;
        self.ja4_fp = None;
    }
}
