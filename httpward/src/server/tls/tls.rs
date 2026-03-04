use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::sync::{Arc, Once};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use rama::tls::rustls::{
    dep::pemfile,
};
use rama_tls_rustls::server::{TlsAcceptorData, TlsAcceptorDataBuilder};
use rama_tls_rustls::dep::rustls::{
    pki_types::{CertificateDer, PrivateKeyDer},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
    ServerConfig,
};
use tracing::{info, warn, error};

use crate::runtime::server_instance::TlsMapping;
use super::tls_watcher::TlsFileWatcher;

/// Global flag to ensure crypto provider is installed only once
static CRYPTO_PROVIDER_INSTALLED: AtomicBool = AtomicBool::new(false);
static CRYPTO_PROVIDER_INIT: Once = Once::new();


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

        // Install the ring crypto provider as default (required for rustls) - only once!
        CRYPTO_PROVIDER_INIT.call_once(|| {
            match rustls::crypto::ring::default_provider()
                .install_default() {
                Ok(_) => {
                    CRYPTO_PROVIDER_INSTALLED.store(true, Ordering::SeqCst);
                    info!("Ring crypto provider installed successfully");
                }
                Err(e) => {
                    warn!("Failed to install ring crypto provider: {:?}", e);
                }
            }
        });

        // Check if provider was installed successfully
        if !CRYPTO_PROVIDER_INSTALLED.load(Ordering::SeqCst) {
            return Err("Failed to install ring crypto provider".into());
        }

        // Load all certificates and build SNI resolver with fallback
        let (domain_to_cert, fallback_cert, fallback_domain) = self.build_tls_mappings().await?;

        let resolver = Arc::new(FallbackSniResolver::new(
            domain_to_cert,
            fallback_cert,
            fallback_domain,
        ));

        // Watcher to update certificates if they were changed
        let watcher = TlsFileWatcher::new(
            self.mappings.clone(), 
            resolver.clone(),
        ).with_debounce_delay(std::time::Duration::from_millis(1000));
        
        tokio::spawn(async move {
            if let Err(e) = watcher.run().await {
                error!("TLS file watcher error: {:?}", e);
            }
        });

        
        // Build rustls server config with custom resolver
        let mut server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(resolver.clone());

        // Set ALPN protocols for HTTP/1.1 and HTTP/2
        server_config.alpn_protocols = vec![
            b"h2".to_vec(),
            b"http/1.1".to_vec(),
        ];

        // Convert to Rama's TlsAcceptorDataBuilder and build
        let tls_data = TlsAcceptorDataBuilder::from(server_config).build();

        Ok(tls_data)
    }

    /// Build TLS mappings from certificate configurations
    async fn build_tls_mappings(&self) -> Result<(
        Arc<RwLock<std::collections::HashMap<String, Arc<CertifiedKey>>>>,
        Arc<CertifiedKey>,
        String,
    ), TlsError> {
        let mut domain_to_cert: std::collections::HashMap<String, Arc<CertifiedKey>> = std::collections::HashMap::new();
        let mut fallback_cert: Option<Arc<CertifiedKey>> = None;
        let mut fallback_domain: Option<String> = None;

        for mapping in &self.mappings {
            let certified_key = self.build_single_mapping(mapping).await?;

            // Set as fallback if it's the first certificate
            if fallback_cert.is_none() {
                fallback_cert = Some(certified_key.clone());
                // Use the first domain as fallback domain
                if let Some(first_domain) = mapping.domains.first() {
                    fallback_domain = Some(first_domain.to_lowercase());
                }
                info!("Set fallback TLS certificate for domain: {:?}", fallback_domain);
            }

            // Add to domain map for each domain
            for domain in &mapping.domains {
                let domain_lower = domain.to_lowercase();
                domain_to_cert.insert(domain_lower.clone(), certified_key.clone());
                info!("Added TLS certificate for domain: {}", domain);
            }
        }

        Ok((
            Arc::new(RwLock::new(domain_to_cert)),
            fallback_cert.ok_or("No fallback certificate available")?,
            fallback_domain.ok_or("No fallback domain available")?,
        ))
    }

    /// Build a single TLS mapping from certificate configuration
    async fn build_single_mapping(&self, mapping: &TlsMapping) -> Result<Arc<CertifiedKey>, TlsError> {
        build_certified_key(&mapping.paths.cert, &mapping.paths.key).await
    }
}

/// Custom SNI resolver with fallback to first certificate and thread-safe updates
#[derive(Debug)]
pub struct FallbackSniResolver {
    domain_to_cert: Arc<RwLock<std::collections::HashMap<String, Arc<CertifiedKey>>>>,
    fallback_cert: Arc<RwLock<Arc<CertifiedKey>>>,
    fallback_domain: Arc<RwLock<String>>,
}

impl FallbackSniResolver {
    /// Create a new SNI resolver with the given mappings
    fn new(
        domain_to_cert: Arc<RwLock<std::collections::HashMap<String, Arc<CertifiedKey>>>>,
        fallback_cert: Arc<CertifiedKey>,
        fallback_domain: String,
    ) -> Self {
        Self {
            domain_to_cert,
            fallback_cert: Arc::new(RwLock::new(fallback_cert)),
            fallback_domain: Arc::new(RwLock::new(fallback_domain)),
        }
    }

    /// Update or add a certificate for a specific domain
    pub fn update_domain_certificate(&self, domain: Option<&String>, cert: Arc<CertifiedKey>) {
        if let Some(domain_ref) = domain {
            let domain_lower = domain_ref.to_lowercase();
            let mut domain_map = self.domain_to_cert.write().unwrap();
            
            // Check if domain already exists in the map
            if domain_map.contains_key(&domain_lower) {
                // Update existing certificate
                domain_map.insert(domain_lower.clone(), cert.clone());
                info!("Updated existing TLS certificate for domain: {}", domain_ref);
                
                // Check if this domain is the current fallback domain and update it if needed
                let current_fallback_domain = self.fallback_domain.read().unwrap();
                if *current_fallback_domain == domain_lower {
                    drop(current_fallback_domain); // Release read lock before acquiring write lock
                    let mut fallback_cert = self.fallback_cert.write().unwrap();
                    *fallback_cert = cert.clone();
                    info!("Also updated fallback certificate for domain: {}", domain_ref);
                }
            } else {
                warn!("Domain {} not found in certificate mappings - not added", domain_ref);
            }
        }
    }

    /// Get the current fallback domain
    pub fn get_fallback_domain(&self) -> String {
        self.fallback_domain.read().unwrap().clone()
    }
}

impl ResolvesServerCert for FallbackSniResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        // Extract SNI first before converting ClientHello (to avoid borrow issue)
        let server_name = client_hello.server_name().map(|s| s.to_lowercase());

        // Try to find certificate by SNI
        if let Some(domain) = server_name {
            let domain_map = self.domain_to_cert.read().unwrap();
            if let Some(cert) = domain_map.get(&domain) {
                info!("Resolved TLS certificate for domain: {}", domain);
                return Some(cert.clone());
            }
        }

        // Return fallback certificate when no SNI match
        let fallback_cert = self.fallback_cert.read().unwrap();
        info!("No SNI match, using fallback certificate for domain: {}", self.get_fallback_domain());
        Some(fallback_cert.clone())
    }
}

/// Build a certified key from certificate chain and private key
pub async fn build_certified_key(cert_path: &Path, key_path: &Path) -> Result<Arc<CertifiedKey>, TlsError> {
    let cert_chain = load_cert_chain(cert_path).await?;
    let key = load_private_key(key_path).await?;

    let certified_key = Arc::new(CertifiedKey::new(
        cert_chain,
        rustls::crypto::ring::sign::any_supported_type(&key)?
    ));

    Ok(certified_key)
}

/// Load certificate chain from PEM file
pub async fn load_cert_chain(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
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
pub async fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
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
