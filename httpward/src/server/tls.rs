use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use rama::net::{
    fingerprint::Ja4,
    tls::{
        client::{ClientHello as RamaClientHello, ClientHelloExtension},
        CipherSuite, CompressionAlgorithm, ProtocolVersion, SignatureScheme,
    },
};
use rama::tls::rustls::{
    server::{TlsAcceptorDataBuilder, TlsAcceptorData},
    dep::pemfile,
};
use rama_tls_rustls::{
    dep::rustls::{
        server::{ClientHello, ResolvesServerCert},
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer},
        sign::CertifiedKey,
    },
    RamaFrom,
};
use tracing::{info, warn};

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

    /// Build TLS acceptor data with SNI support for multiple domains and fallback
    pub async fn build(self) -> Result<TlsAcceptorData, TlsError> {
        if self.mappings.is_empty() {
            return Err("No TLS mappings available".into());
        }

        // Install the ring crypto provider as default (required for rustls)
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| "Failed to install ring crypto provider")?;

        // Load all certificates and build SNI resolver with fallback
        let mut domain_to_cert: std::collections::HashMap<String, Arc<CertifiedKey>> = std::collections::HashMap::new();
        let mut fallback_cert: Option<Arc<CertifiedKey>> = None;

        for mapping in &self.mappings {
            let cert_chain = load_cert_chain(&mapping.paths.cert).await?;
            let key = load_private_key(&mapping.paths.key).await?;

            // Create certified key
            let certified_key = Arc::new(CertifiedKey::new(
                cert_chain,
                rustls::crypto::ring::sign::any_supported_type(&key)?
            ));

            // Set as fallback if it's the first certificate
            if fallback_cert.is_none() {
                fallback_cert = Some(certified_key.clone());
                info!("Set fallback TLS certificate");
            }

            // Add to domain map for each domain
            for domain in &mapping.domains {
                let domain_lower = domain.to_lowercase();
                domain_to_cert.insert(domain_lower.clone(), certified_key.clone());
                info!("Added TLS certificate for domain: {}", domain);
            }
        }

        let resolver = FallbackSniResolver {
            domain_to_cert,
            fallback_cert: fallback_cert.ok_or("No fallback certificate available")?,
        };

        // Build rustls server config with custom resolver
        let mut server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(resolver));

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

/// Custom SNI resolver with fallback to first certificate
#[derive(Debug)]
struct FallbackSniResolver {
    domain_to_cert: std::collections::HashMap<String, Arc<CertifiedKey>>,
    fallback_cert: Arc<CertifiedKey>,
}

impl ResolvesServerCert for FallbackSniResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        // Extract SNI first before converting ClientHello (to avoid borrow issue)
        let server_name = client_hello.server_name().map(|s| s.to_lowercase());

        // Compute JA4 fingerprint from ClientHello manually
        // Extract data from rustls ClientHello and build Rama ClientHello
        let cipher_suites: Vec<CipherSuite> = client_hello
            .cipher_suites()
            .iter()
            .map(|cs| CipherSuite::try_from(u16::from(*cs)).unwrap_or(CipherSuite::Unknown(0)))
            .collect();

        let signature_schemes: Vec<SignatureScheme> = client_hello
            .signature_schemes()
            .iter()
            .map(|ss| SignatureScheme::try_from(u16::from(*ss)).unwrap_or(SignatureScheme::Unknown(0)))
            .collect();

        // Build extensions (simplified - only signature algorithms)
        let mut extensions: Vec<ClientHelloExtension> = Vec::new();
        
        // Add signature algorithms extension
        if !signature_schemes.is_empty() {
            extensions.push(ClientHelloExtension::SignatureAlgorithms(signature_schemes));
        }

        // Create ClientHello with TLS 1.2 version (commonly supported)
        let rama_client_hello = RamaClientHello::new(
            ProtocolVersion::TLSv1_2,
            cipher_suites,
            vec![CompressionAlgorithm::Null],
            extensions,
        );

        // Try to compute JA4 fingerprint
        match Ja4::compute_from_client_hello(&rama_client_hello, None) {
            Ok(ja4) => {
                let ja4_str = ja4.to_string();
                info!("JA4 fingerprint: {}", ja4_str);
            }
            Err(e) => {
                warn!("Failed to compute JA4 fingerprint: {}", e);
            }
        }

        // Try to find certificate by SNI
        if let Some(domain) = server_name {
            if let Some(cert) = self.domain_to_cert.get(&domain) {
                info!("Resolved TLS certificate for domain: {}", domain);
                return Some(cert.clone());
            }
        }

        // Return fallback certificate (first in registry) when no SNI match
        info!("No SNI match, using fallback certificate");
        Some(self.fallback_cert.clone())
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
