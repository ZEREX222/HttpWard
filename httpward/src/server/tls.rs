use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use rama::tls::rustls::{
    server::{TlsAcceptorDataBuilder, TlsAcceptorData},
    dep::pemfile,
};
use rustls::{
    ServerConfig,
    server::ResolvesServerCertUsingSni,
    pki_types::{CertificateDer, PrivateKeyDer},
    sign::CertifiedKey,
};
use tracing::info;

use crate::runtime::server_instance::TlsMapping;

/// Error type for TLS operations
pub type TlsError = Box<dyn std::error::Error + Send + Sync>;

/// TLS configuration builder for HttpWard server
pub struct TlsConfigBuilder {
    mappings: Vec<TlsMapping>,
}

impl TlsConfigBuilder {
    /// Create a new TLS config builder with the given certificate mappings
    pub fn new(mappings: Vec<TlsMapping>) -> Self {
        Self { mappings }
    }

    /// Build TLS acceptor data with SNI support for multiple domains
    pub async fn build(self) -> Result<TlsAcceptorData, TlsError> {
        if self.mappings.is_empty() {
            return Err("No TLS mappings available".into());
        }

        // Install the ring crypto provider as default (required for rustls)
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| "Failed to install ring crypto provider")?;

        // Load certificates for all domains
        let mut sni_resolver = ResolvesServerCertUsingSni::new();

        for mapping in &self.mappings {
            let cert_chain = load_cert_chain(&mapping.paths.cert).await?;
            let key = load_private_key(&mapping.paths.key).await?;

            // Create certified key
            let certified_key = CertifiedKey::new(
                cert_chain,
                rustls::crypto::ring::sign::any_supported_type(&key)?
            );

            // Add to SNI resolver for each domain
            for domain in &mapping.domains {
                let domain_lower = domain.to_lowercase();
                sni_resolver.add(domain_lower.as_str(), certified_key.clone())?;
                info!("Added TLS certificate for domain: {}", domain);
            }
        }

        // Build rustls server config with SNI resolver
        let mut server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(sni_resolver));

        // Set ALPN protocols for HTTP/1.1 and HTTP/2
        server_config.alpn_protocols = vec![
            b"h2".to_vec(),
            b"http/1.1".to_vec(),
        ];

        // Convert to Rama's TlsAcceptorDataBuilder and build
        let tls_data = TlsAcceptorDataBuilder::from(server_config).build();

        Ok(tls_data)
    }
}

/// Load certificate chain from PEM file
async fn load_cert_chain(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let content = fs::read(path)?;
    let mut reader = BufReader::new(&content[..]);

    let certs: Vec<CertificateDer<'static>> = pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to parse certificate from {:?}: {}", path, e))?;

    if certs.is_empty() {
        return Err(format!("No certificates found in {:?}", path).into());
    }

    Ok(certs)
}

/// Load private key from PEM file
async fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
    let content = fs::read(path)?;
    let mut reader = BufReader::new(&content[..]);

    // Try reading as PKCS8 first
    if let Some(key) = pemfile::pkcs8_private_keys(&mut reader)
        .next()
        .and_then(|r| r.ok())
    {
        return Ok(PrivateKeyDer::try_from(key)?);
    }

    // Reset reader and try RSA format
    let mut reader = BufReader::new(&content[..]);
    if let Some(key) = pemfile::rsa_private_keys(&mut reader)
        .next()
        .and_then(|r| r.ok())
    {
        return Ok(PrivateKeyDer::try_from(key)?);
    }

    // Try one more time with EC format (SEC1 keys)
    let mut reader = BufReader::new(&content[..]);
    if let Some(key) = pemfile::ec_private_keys(&mut reader)
        .next()
        .and_then(|r: Result<rustls::pki_types::PrivateSec1KeyDer<'static>, std::io::Error>| r.ok())
    {
        return Ok(PrivateKeyDer::try_from(key)?);
    }

    Err(format!("No valid private key found in {:?}", path).into())
}
