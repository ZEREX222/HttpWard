use rama::{graceful::Shutdown, http::{
    server::HttpServer, Body, Request, Response,
    StatusCode,
}, layer::Layer, net::address::SocketAddress, rt::Executor, service::service_fn, tcp::server::TcpListener, tls::rustls::server::TlsAcceptorLayer, Context};
use rama::net::fingerprint::Ja4;
use rama::net::tls::{ProtocolVersion, SecureTransport};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::runtime::server_instance::ServerInstance;
use httpward_core::middleware::{LogLayer, RequestEnricherLayer, ResponseEnricherLayer, HttpWardContext};
use crate::server::tls::tls::TlsConfigBuilder;

/// HttpWard HTTP/TLS server
pub struct HttpWardServer {
    pub instance: ServerInstance,
}

impl HttpWardServer {
    /// Create a new server instance
    pub fn new(instance: ServerInstance) -> Self {
        Self { instance }
    }

    /// Run the server with graceful shutdown support
    pub async fn run(&self, shutdown: Shutdown) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let host = &self.instance.bind.host;
        let port = self.instance.bind.port;
        let addr_str = format!("{}:{}", host, port);

        // Parse the address
        let addr: SocketAddress = match addr_str.parse() {
            Ok(addr) => addr,
            Err(_) => {
                // If it's a hostname, try to bind to 0.0.0.0 with the port
                format!("0.0.0.0:{}", port)
                    .parse()
                    .map_err(|e| format!("Invalid address: {}", e))?
            }
        };

        let display_addr = addr.to_string().replace("0.0.0.0", "127.0.0.1");

        let exec = Executor::graceful(shutdown.guard());
        let tls_enabled = !self.instance.tls_registry.is_empty();

        // Create HTTP service with dynamic middleware layers using LayerStackBuilder
        let base_service = service_fn(move |ctx: Context<()>, _req: Request<Body>| {
            async move {

                if let Some(req_ctx) = ctx.get::<HttpWardContext>() {
                     error!("CONTENT TYPE!: {:?}", req_ctx.request_content_type);
                 }

                // Try to get SecureTransport from context
                if let Some(st) = ctx.get::<SecureTransport>() {

                    // ClientHello is available only if with_store_client_hello(true) was enabled
                    if let Some(client_hello) = st.client_hello() {

                        let pv = client_hello.protocol_version();

                        let effective_version = match pv {
                            ProtocolVersion::Unknown(_) => ProtocolVersion::TLSv1_2,
                            other => other,
                        };

                        // Try to compute JA4 fingerprint
                        match Ja4::compute_from_client_hello(client_hello, Some(effective_version)) {
                            Ok(ja4) => {
                                let ja4_str = ja4.to_string();
                                info!("JA4 fingerprint: {}", ja4_str);
                            }
                            Err(e) => {
                                warn!("Failed to compute JA4 fingerprint: {}", e);
                            }
                        }

                    }
                }

                let response = Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain")
                    .body(Body::from("Backend Reached (Rama Server)"))
                    .unwrap();

                Ok::<_, std::convert::Infallible>(response)
            }
        });

        

        // Use LayerStackBuilder for dynamic middleware composition
        // Create Arc references for configs to avoid duplication
        let sites_arc: Vec<Arc<httpward_core::config::SiteConfig>> = self.instance.sites
            .iter()
            .map(|site| Arc::new(site.clone()))
            .collect();
        let global_arc = Arc::new(self.instance.global.clone());
        
        let http_svc = HttpServer::auto(exec.clone()).service(
            (
                RequestEnricherLayer::new(sites_arc, global_arc),
                LogLayer::new(),
                ResponseEnricherLayer::new(),
            )
                .into_layer(base_service)
        );

        // Bind TCP listener and serve
        let listener = TcpListener::bind(addr).await
            .map_err(|e| format!("Failed to bind server on {}: {}", addr, e))?;

        if tls_enabled {
            info!("TLS enabled for server on https://{}", display_addr);

            // Build TLS acceptor data with SNI support
            let tls_config = TlsConfigBuilder::new(self.instance.tls_registry.clone());
            let tls_data = tls_config.build().await
                .map_err(|e| format!("Failed to build TLS config: {}", e))?;

            // Serve connections with TLS
            listener
                .serve(TlsAcceptorLayer::new(tls_data).with_store_client_hello(true).into_layer(http_svc))
                .await;
        } else {
            warn!("TLS not configured. Server running on http://{}", display_addr);

            // Serve connections without TLS
            listener.serve(http_svc).await;
        }

        Ok(())
    }
}
