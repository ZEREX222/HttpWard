use std::fs;
use std::io::BufReader;
use std::sync::Arc;

use rama::{
    graceful::Shutdown,
    http::{
        Request, Response, StatusCode, Body,
        server::HttpServer,
    },
    layer::Layer,
    net::address::SocketAddress,
    rt::Executor,
    service::service_fn,
    tcp::server::TcpListener,
    tls::rustls::{
        server::{TlsAcceptorDataBuilder, TlsAcceptorLayer},
        dep::pemfile,
    },
};
use rustls::{
    ServerConfig,
    server::ResolvesServerCertUsingSni,
    pki_types::{CertificateDer, PrivateKeyDer},
    sign::CertifiedKey,
};
use tracing::{error, info, warn};

use httpward_core::middleware::{LogLayer};
use crate::runtime::server_instance::ServerInstance;

pub struct HttpWardServer {
    pub instance: ServerInstance,
}

impl HttpWardServer {
    pub fn new(instance: ServerInstance) -> Self {
        Self { instance }
    }

    pub async fn run(&self, shutdown: Shutdown) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let host = self.instance.bind.host.clone();
        let port = self.instance.bind.port;
        let addr_str = format!("{}:{}", host, port);

        // Parse the address
        let addr: SocketAddress = match addr_str.parse() {
            Ok(addr) => addr,
            Err(_) => {
                // If it's a hostname, try to bind to 0.0.0.0 with the port
                match format!("0.0.0.0:{}", port).parse() {
                    Ok(addr) => addr,
                    Err(e) => return Err(format!("Invalid address: {}", e).into()),
                }
            }
        };

        let display_addr = addr_str.replace("0.0.0.0", "127.0.0.1");
        info!("📡 Starting HttpServer on {}", display_addr);

        // Create executor with graceful shutdown guard
        let exec = Executor::graceful(shutdown.guard());

        // Build TLS configuration if TLS mappings exist
        let tls_enabled = !self.instance.tls_registry.is_empty();

        // Create the HTTP service with logging middleware
        let http_svc = HttpServer::auto(exec.clone()).service(
            LogLayer::new().layer(
                service_fn(move |_req: Request<Body>| {
                    async move {
                        // Default handler - returns 200 OK
                        let response = Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/plain")
                            .body(Body::from("Backend Reached (Rama Server)"))
                            .unwrap();

                        Ok::<_, std::convert::Infallible>(response)
                    }
                })
            )
        );

        // Bind TCP listener and serve
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                info!("Server listening on {}", display_addr);

                if tls_enabled {
                    info!("TLS enabled for server on https://{}", display_addr);

                    // Build TLS acceptor data with SNI support
                    let tls_data = self.build_tls_data().await
                        .map_err(|e| format!("Failed to build TLS config: {}", e))?;

                    // Serve connections with TLS
                    listener
                        .serve(
                            TlsAcceptorLayer::new(tls_data)
                                .into_layer(http_svc)
                        )
                        .await;
                } else {
                    warn!("TLS not configured. Server running on http://{}", display_addr);

                    // Serve connections without TLS
                    listener
                        .serve(http_svc)
                        .await;
                }

                Ok(())
            }
            Err(e) => {
                error!("Failed to bind server on {}: {}", addr, e);
                Err(format!("Bind error: {}", e).into())
            }
        }
    }

    /// Build TLS acceptor data with SNI support for multiple domains
    async fn build_tls_data(&self) -> Result<rama::tls::rustls::server::TlsAcceptorData, Box<dyn std::error::Error + Send + Sync>> {
        let tls_registry = &self.instance.tls_registry;

        if tls_registry.is_empty() {
            return Err("No TLS mappings available".into());
        }

        // Install the ring crypto provider as default (required for rustls)
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| "Failed to install ring crypto provider")?;

        // Load certificates for all domains
        let mut sni_resolver = ResolvesServerCertUsingSni::new();

        for mapping in tls_registry {
            let cert_chain = self.load_cert_chain(&mapping.paths.cert).await?;
            let key = self.load_private_key(&mapping.paths.key).await?;

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

    /// Load certificate chain from PEM file
    async fn load_cert_chain(&self, path: &std::path::Path) -> Result<Vec<CertificateDer<'static>>, Box<dyn std::error::Error + Send + Sync>> {
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
    async fn load_private_key(&self, path: &std::path::Path) -> Result<PrivateKeyDer<'static>, Box<dyn std::error::Error + Send + Sync>> {
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
        reader = BufReader::new(&content[..]);
        if let Some(key) = pemfile::rsa_private_keys(&mut reader)
            .next()
            .and_then(|r| r.ok())
        {
            return Ok(PrivateKeyDer::try_from(key)?);
        }

        // Try one more time with sec1 format (EC keys)
        reader = BufReader::new(&content[..]);
        if let Some(key) = pemfile::ec_private_keys(&mut reader)
            .next()
            .and_then(|r: Result<rustls::pki_types::PrivateSec1KeyDer<'static>, std::io::Error>| r.ok())
        {
            return Ok(PrivateKeyDer::try_from(key)?);
        }

        Err(format!("No valid private key found in {:?}", path).into())
    }
}
