use rama_tls_rustls::dep::rustls::sign::CertifiedKey;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};
use wildmatch::WildMatch;

/// Certificate wrapper for DomainStore
#[derive(Clone, Debug)]
pub struct Cert {
    pub certified_key: Arc<CertifiedKey>,
}

/// Domain store with support for exact and wildcard domain matching
///
/// This structure provides efficient domain lookup with support for:
/// - Exact domain matching (highest priority)
/// - Wildcard pattern matching (e.g., "*.example.com")
/// - Multiple certificates per domain/pattern
/// - Fast O(1) exact lookups, O(n) wildcard lookups
///
/// # Examples
///
/// ```rust
/// let mut store = DomainStore::new();
///
/// // Add exact domain
/// store.insert("example.com", cert1);
///
/// // Add wildcard domain
/// store.insert("*.example.com", cert2);
///
/// // Find certificate for domain
/// let cert = store.find_first("test.example.com");
///
/// // Get fallback certificate
/// let fallback = store.first_cert();
/// ```
#[derive(Debug)]
pub struct DomainStore {
    exact: HashMap<String, Vec<Cert>>,
    wildcards: Vec<(WildMatch, Vec<Cert>)>,
}

impl DomainStore {
    pub fn new() -> Self {
        Self {
            exact: HashMap::new(),
            wildcards: Vec::new(),
        }
    }

    pub fn insert(&mut self, domain: &str, cert: Cert) {
        if domain.contains('*') {
            for (pattern, certs) in &mut self.wildcards {
                if pattern.matches(domain) {
                    certs.push(cert);
                    return;
                }
            }

            self.wildcards.push((WildMatch::new(domain), vec![cert]));
        } else {
            self.exact
                .entry(domain.to_string())
                .or_default()
                .push(cert);
        }
    }

    pub fn find(&self, host: &str) -> Option<&Vec<Cert>> {
        if let Some(certs) = self.exact.get(host) {
            return Some(certs);
        }

        for (pattern, certs) in &self.wildcards {
            if pattern.matches(host) {
                return Some(certs);
            }
        }

        None
    }

    pub fn find_first(&self, host: &str) -> Option<&Cert> {
        if let Some(certs) = self.exact.get(host) {
            return certs.first();
        }

        for (pattern, certs) in &self.wildcards {
            if pattern.matches(host) {
                return certs.first();
            }
        }

        None
    }

    /// Get the first available certificate (fallback)
    ///
    /// Returns the first certificate found, prioritizing exact domains over wildcards.
    /// Useful for getting a fallback certificate when no specific domain matches.
    pub fn first_cert(&self) -> Option<&Cert> {
        if let Some((_, certs)) = self.exact.iter().next()
            && let Some(cert) = certs.first() {
                return Some(cert);
            }

        for (_, certs) in &self.wildcards {
            if let Some(cert) = certs.first() {
                return Some(cert);
            }
        }

        None
    }

    /// Get the first available certificate with its associated domain
    ///
    /// Returns both the domain/pattern and the certificate, prioritizing exact domains
    /// over wildcards. Useful for logging and debugging when you need to know which
    /// domain is being used as fallback.
    ///
    /// # Returns
    ///
    /// `Some((domain, cert))` - The domain/pattern and associated certificate
    /// `None` - No certificates available
    pub fn first_cert_with_domain(&self) -> Option<(String, &Cert)> {
        // Check exact domains first
        if let Some((domain, certs)) = self.exact.iter().next()
            && let Some(cert) = certs.first() {
                return Some((domain.clone(), cert));
            }

        // Check wildcard patterns
        for (pattern, certs) in &self.wildcards {
            if let Some(cert) = certs.first() {
                // Convert WildMatch to string representation
                return Some((pattern.to_string(), cert));
            }
        }

        None
    }

    // Update certificate for existing domain/pattern
    pub fn update_cert(&mut self, domain: &str, new_cert: Cert) -> bool {
        let domain_lower = domain.to_lowercase();

        // Check exact domains first
        if let Some(certs) = self.exact.get_mut(&domain_lower)
            && !certs.is_empty() {
                certs[0] = new_cert; // Replace first certificate
                info!("Updated certificate for exact domain: {}", domain_lower);
                return true;
            }

        // Check wildcard patterns
        for (pattern, certs) in &mut self.wildcards {
            if pattern.matches(&domain_lower) && !certs.is_empty() {
                certs[0] = new_cert; // Replace first certificate
                info!(
                    "Updated certificate for wildcard pattern matching: {}",
                    domain_lower
                );
                return true;
            }
        }

        warn!("Domain {} not found for certificate update", domain_lower);
        false
    }

    // Remove all certificates for a domain
    pub fn remove_domain(&mut self, domain: &str) -> bool {
        let domain_lower = domain.to_lowercase();

        // Remove from exact domains
        if self.exact.remove(&domain_lower).is_some() {
            info!("Removed exact domain: {}", domain_lower);
            return true;
        }

        // Remove from wildcards
        let initial_len = self.wildcards.len();
        self.wildcards
            .retain(|(pattern, _)| !pattern.matches(&domain_lower));
        let removed = self.wildcards.len() < initial_len;

        if removed {
            info!("Removed wildcard patterns matching: {}", domain_lower);
        }

        removed
    }

    // Update certificates for multiple domains at once
    pub fn update_domains(&mut self, domains: &[String], new_cert: Cert) {
        let domains_lower: Vec<String> = domains.iter().map(|d| d.to_lowercase()).collect();

        // Remove old entries for these domains
        for domain in &domains_lower {
            self.remove_domain(domain);
        }

        // Add new certificate for all domains
        for domain in &domains_lower {
            self.update_cert(domain, new_cert.clone());
        }

        info!("Updated certificate for domains: {:?}", domains_lower);
    }
}
