use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};

use rama::tls::rustls::dep::pemfile;
use rama_tls_rustls::dep::rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};
use rama_tls_rustls::server::{TlsAcceptorData, TlsAcceptorDataBuilder};
use tracing::{error, info, warn};

use super::tls_watcher::TlsWatcherManager;
use crate::server::tls::domain_store::{Cert, DomainStore};
use httpward_core::core::server_models::site_manager::TlsMapping;

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
            match rustls::crypto::ring::default_provider().install_default() {
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

        // Create resolver directly from mappings using DomainStore
        let resolver = Arc::new(FallbackSniResolver::from_mappings(self.mappings.clone()).await?);

        // Register mappings with global watcher manager to ensure unique watchers
        let watcher_manager = TlsWatcherManager::instance();
        if let Err(e) = watcher_manager
            .register_mappings(self.mappings.clone(), resolver.clone())
            .await
        {
            error!(
                "Failed to register TLS mappings with watcher manager: {:?}",
                e
            );
        }

        // Build rustls server config with custom resolver
        let mut server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(resolver.clone());

        // Set ALPN protocols for HTTP/1.1 and HTTP/2
        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        // Convert to Rama's TlsAcceptorDataBuilder and build
        let tls_data = TlsAcceptorDataBuilder::from(server_config).build();

        Ok(tls_data)
    }
}

/// Custom SNI resolver with DomainStore for exact and wildcard domain matching
///
/// This resolver supports:
/// - Exact domain matching (e.g., "example.com")
/// - Wildcard domain matching (e.g., "*.example.com", "*.sub.example.com")
/// - Multiple certificates per domain
/// - Automatic fallback to first available certificate
///
/// # Examples
///
/// ```rust
/// // Create resolver from TLS mappings
/// let resolver = FallbackSniResolver::from_mappings(mappings).await?;
///
/// // Update certificate for a domain
/// resolver.update_domain_certificate(Some(&"example.com".to_string()), cert);
///
/// // Use with rustls ServerConfig
/// let server_config = ServerConfig::builder()
///     .with_no_client_auth()
///     .with_cert_resolver(Arc::new(resolver));
/// ```
///
/// # Note
///
/// This resolver uses DomainStore internally and doesn't require separate fallback
/// certificate/domain management - fallback is automatically handled by DomainStore.first_cert()
#[derive(Debug)]
pub struct FallbackSniResolver {
    domain_store: Arc<RwLock<DomainStore>>,
}

impl FallbackSniResolver {
    /// Create a new SNI resolver directly from TLS mappings (preferred method)
    pub async fn from_mappings(mappings: Vec<TlsMapping>) -> Result<Self, TlsError> {
        let domain_store = Arc::new(RwLock::new(DomainStore::new()));

        for mapping in mappings {
            let certified_key =
                build_certified_key(&mapping.paths.cert, &mapping.paths.key).await?;
            let cert = Cert { certified_key };

            let mut store = domain_store.write().unwrap();
            for domain in &mapping.domains {
                let domain_str: &str = &domain.to_lowercase();
                store.insert(domain_str, cert.clone());
            }
        }

        Ok(Self { domain_store })
    }

    /// Update or add a certificate for a specific domain
    pub fn update_domain_certificate(&self, domain: Option<&String>, cert: Arc<CertifiedKey>) {
        if let Some(domain_ref) = domain {
            let domain_lower = domain_ref.to_lowercase();
            let cert = Cert {
                certified_key: cert,
            };

            let mut store = self.domain_store.write().unwrap();

            // Try to update existing certificate first
            if store.update_cert(&domain_lower, cert.clone()) {
                info!(
                    "Updated existing TLS certificate for domain: {}",
                    domain_ref
                );
            } else {
                // Insert new certificate if not found
                store.insert(&domain_lower, cert);
                info!("Added new TLS certificate for domain: {}", domain_ref);
            }
        }
    }

    /// Update certificate for multiple domains at once
    pub fn update_domains_certificate(&self, domains: Vec<String>, cert: Arc<CertifiedKey>) {
        let cert = Cert {
            certified_key: cert,
        };

        let mut store = self.domain_store.write().unwrap();
        store.update_domains(&domains, cert);

        info!(
            "Updated TLS certificate for multiple domains: {:?}",
            domains
        );
    }
}

impl ResolvesServerCert for FallbackSniResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let server_name = client_hello.server_name().map(|s| s.to_lowercase());
        let store = self.domain_store.read().unwrap();

        if let Some(domain) = server_name {
            // Try to find certificate by exact or wildcard match
            if let Some(cert) = store.find_first(&domain) {
                info!("Resolved TLS certificate for domain: {}", domain);
                return Some(cert.certified_key.clone());
            }
        }

        // Return fallback certificate when no SNI match
        if let Some((domain, cert)) = store.first_cert_with_domain() {
            info!(
                "No SNI match, using fallback certificate for domain: {}",
                domain
            );
            Some(cert.certified_key.clone())
        } else {
            error!("No certificates available for TLS resolution");
            None
        }
    }
}

/// Build a certified key from certificate chain and private key
pub async fn build_certified_key(
    cert_path: &Path,
    key_path: &Path,
) -> Result<Arc<CertifiedKey>, TlsError> {
    let cert_chain = load_cert_chain(cert_path).await?;
    let key = load_private_key(key_path).await?;

    let certified_key = Arc::new(CertifiedKey::new(
        cert_chain,
        rustls::crypto::ring::sign::any_supported_type(&key)?,
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
        return Ok(PrivateKeyDer::from(key));
    }

    // Reset reader and try RSA format
    let mut reader = BufReader::new(&content[..]);
    if let Some(key) = pemfile::rsa_private_keys(&mut reader)
        .next()
        .and_then(|r| r.ok())
    {
        return Ok(PrivateKeyDer::from(key));
    }

    // Try one more time with EC format (SEC1 keys)
    let mut reader = BufReader::new(&content[..]);
    if let Some(key) = pemfile::ec_private_keys(&mut reader)
        .next()
        .and_then(|r: Result<rustls::pki_types::PrivateSec1KeyDer<'static>, std::io::Error>| r.ok())
    {
        return Ok(PrivateKeyDer::from(key));
    }

    Err(format!("No valid private key found in {:?}", path).into())
}
