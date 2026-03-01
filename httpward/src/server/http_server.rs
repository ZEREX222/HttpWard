use tokio::net::TcpListener;
use std::sync::Arc;
use std::fs::File;
use std::io::BufReader;
use std::collections::HashMap;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use http_body_util::{Full, Either};
use hyper::body::Bytes;
use tracing::{error, info, warn};

// TLS & SNI imports
use tokio_rustls::rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer},
    server::{ResolvesServerCert, ClientHello},
    sign::CertifiedKey,
};
use tokio_rustls::TlsAcceptor;

use httpward_core::middleware::{Middleware, MiddlewareFuture, RequestContext};
use crate::runtime::server_instance::ServerInstance;

/// Custom resolver using SitePlan mapping for SNI
#[derive(Debug)]
struct SniResolver {
    cert_map: HashMap<String, Arc<CertifiedKey>>,
}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        if let Some(name) = client_hello.server_name() {
            if let Some(cert) = self.cert_map.get(name) {
                return Some(Arc::clone(cert));
            }
        }
        // Fallback to the first available certificate in the map
        self.cert_map.values().next().cloned()
    }
}

pub struct HttpServer {
    pub instance: ServerInstance,
    pub pipeline: Arc<Vec<Box<dyn Middleware>>>,
}

impl HttpServer {
    pub fn new(instance: ServerInstance, pipeline: Vec<Box<dyn Middleware>>) -> Self {
        Self {
            instance,
            pipeline: Arc::new(pipeline),
        }
    }

    /// Attempts to build the TLS configuration. Returns Err if no valid certs are found.
    fn create_sni_config(&self) -> Result<ServerConfig, Box<dyn std::error::Error + Send + Sync>> {
        let mut cert_map = HashMap::new();
        let provider = tokio_rustls::rustls::crypto::ring::default_provider();

        for site_plan in &self.instance.sites {
            if let Some(tls) = &site_plan.tls_paths {
                let certs = self.load_certs(&tls.cert)?;
                let key_der = self.load_private_key(&tls.key)?;

                let key_payload = provider.key_provider.load_private_key(key_der)
                    .map_err(|_| format!("Unsupported key format for domain: {}", site_plan.config.domain))?;

                let certified_key = Arc::new(CertifiedKey::new(certs, key_payload));

                // Register primary domain
                if !site_plan.config.domain.is_empty() {
                    cert_map.insert(site_plan.config.domain.clone(), Arc::clone(&certified_key));
                }

                // Register alias domains
                for alias in &site_plan.config.domains {
                    if !alias.is_empty() {
                        cert_map.insert(alias.clone(), Arc::clone(&certified_key));
                    }
                }
            }
        }

        if cert_map.is_empty() {
            return Err("No sites with valid TLS paths found".into());
        }

        let resolver = SniResolver { cert_map };

        let mut config = ServerConfig::builder_with_provider(Arc::new(provider))
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(resolver));

        config.alpn_protocols = vec![b"http/1.1".to_vec()];

        Ok(config)
    }

    fn load_certs(&self, path: &std::path::Path) -> Result<Vec<CertificateDer<'static>>, Box<dyn std::error::Error + Send + Sync>> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
        Ok(certs)
    }

    fn load_private_key(&self, path: &std::path::Path) -> Result<PrivateKeyDer<'static>, Box<dyn std::error::Error + Send + Sync>> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let key = rustls_pemfile::private_key(&mut reader)?
            .ok_or_else(|| format!("Key not found in {:?}", path))?;
        Ok(key)
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr_str = format!("{}:{}", self.instance.bind.host, self.instance.bind.port);
        let addr: std::net::SocketAddr = addr_str.parse().expect("Invalid SocketAddr");
        let listener = TcpListener::bind(addr).await?;

        let display_addr = addr_str.replace("0.0.0.0", "127.0.0.1");

        // Try to initialize TLS. If it fails, we fall back to plain HTTP.
        let tls_acceptor = match self.create_sni_config() {
            Ok(config) => {
                info!("TLS/SNI initialized. Server listening on https://{}", display_addr);
                Some(TlsAcceptor::from(Arc::new(config)))
            }
            Err(e) => {
                warn!("TLS initialization skipped: {}. Falling back to plain HTTP.", e);
                info!("Server listening on http://{}", display_addr);
                None
            }
        };

        loop {
            let (stream, client_addr) = listener.accept().await?;
            let acceptor = tls_acceptor.clone();
            let pipeline = Arc::clone(&self.pipeline);

            tokio::task::spawn(async move {
                if let Some(tls_acceptor) = acceptor {
                    // HTTPS Path
                    match tls_acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            let io = TokioIo::new(tls_stream);
                            serve_connection(io, client_addr, pipeline).await;
                        }
                        Err(e) => {
                            error!("[TLS Handshake Error] {}: {:?}", client_addr, e);
                        }
                    }
                } else {
                    // HTTP Path
                    let io = TokioIo::new(stream);
                    serve_connection(io, client_addr, pipeline).await;
                }
            });
        }
    }
}

/// Helper function to serve the connection via Hyper
async fn serve_connection<I>(
    io: I,
    client_addr: std::net::SocketAddr,
    pipeline: Arc<Vec<Box<dyn Middleware>>>
)
where
    I: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static
{
    let service = service_fn(move |req| {
        let mut ctx = RequestContext::new(client_addr);
        let pipe = Arc::clone(&pipeline);
        async move {
            execute_pipeline(0, pipe, req, &mut ctx).await
        }
    });

    if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
        error!("[Hyper Error] {}: {:?}", client_addr, err);
    }
}

fn execute_pipeline<'a>(
    index: usize,
    pipeline: Arc<Vec<Box<dyn Middleware>>>,
    req: hyper::Request<hyper::body::Incoming>,
    ctx: &'a mut RequestContext,
) -> MiddlewareFuture<'a> {
    Box::pin(async move {
        if index < pipeline.len() {
            let middleware = &pipeline[index];
            let next_pipeline = Arc::clone(&pipeline);
            let next = Box::new(move |next_req, next_ctx| {
                execute_pipeline(index + 1, next_pipeline, next_req, next_ctx)
            });

            middleware.handle(req, ctx, next).await
        } else {
            // Default response if no middleware handles the request
            Ok(hyper::Response::builder()
                .status(200)
                .body(Either::Left(Full::new(Bytes::from("Backend Reached (Multi-protocol)"))))
                .unwrap())
        }
    })
}
