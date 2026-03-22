use serde::{Deserialize, Serialize};

use super::rate_limiter::RateLimitCheckResults;

/// Request-scoped data produced by the rate limit middleware.
///
/// Passed downstream via `HttpwardMiddlewareContext` shared storage under the
/// key `"httpward_rate_limit.context"`.  Retrieve it with
/// `httpward_rate_limit_module::get_rate_limit_context_from_context`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HttpWardRateLimitContext {
    /// Header fingerprint for client identification.
    pub header_fp: Option<String>,
    /// JA4 fingerprint for TLS client identification.
    pub ja4_fp: Option<String>,
    /// Detailed results of every rate-limit check performed for this request.
    pub check_results: Option<RateLimitCheckResults>,
}

impl HttpWardRateLimitContext {
    /// Create new rate limit context with the default 429 status code.
    pub fn new() -> Self {
        Self {
            header_fp: None,
            ja4_fp: None,
            check_results: None,
        }
    }

    pub fn with_header_fp(mut self, header_fp: String) -> Self {
        self.header_fp = Some(header_fp);
        self
    }

    pub fn with_ja4_fp(mut self, ja4_fp: String) -> Self {
        self.ja4_fp = Some(ja4_fp);
        self
    }

    /// Attach the results of `check_all_with_results` so that downstream
    /// middleware (e.g. `HttpWardBlockGatewayLayer`) can act on them.
    pub fn with_check_results(mut self, results: RateLimitCheckResults) -> Self {
        self.check_results = Some(results);
        self
    }

    /// Returns `true` when at least one rate-limit check was `Declined`.
    pub fn is_rate_limited(&self) -> bool {
        self.check_results.as_ref().is_some_and(|r| !r.allowed)
    }
}
