// server/tls_resolver.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio_rustls::rustls::server::{ClientHello, ResolvesServerCert};
use tokio_rustls::rustls::sign::CertifiedKey;

#[derive(Debug)]
pub struct SniResolver {
    pub cert_map: HashMap<String, Arc<CertifiedKey>>,
    /// The certificate to return if SNI is missing or no match is found.
    pub default_cert: Option<Arc<CertifiedKey>>,
}

// In your SniResolver implementation
impl ResolvesServerCert for SniResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        // 1. Attempt to find a certificate by the SNI hostname
        if let Some(name) = client_hello.server_name() {
            if let Some(cert) = self.cert_map.get(name) {
                return Some(Arc::clone(cert));
            }
        }
        self.default_cert.as_ref().map(Arc::clone)
    }
}
