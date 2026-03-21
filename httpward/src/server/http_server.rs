use crate::core::middleware::{
    DynamicModuleLoaderLayer, ErrorHandlerLayer, RequestEnricherLayer, ResponseEnricherLayer,
    RouteLayer,
};
use crate::server::tls::tls::TlsConfigBuilder;
use httpward_core::core::server_models::server_instance::ServerInstance;
use httpward_core::error::ErrorHandler;
use rama::{
    Context,
    graceful::Shutdown,
    http::{Body, Request, Response, StatusCode, server::HttpServer},
    layer::Layer,
    net::address::SocketAddress,
    rt::Executor,
    service::service_fn,
    tcp::server::TcpListener,
    tls::rustls::server::TlsAcceptorLayer,
};
use std::sync::Arc;
use tracing::{info, warn};

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
    pub async fn run(
        &self,
        shutdown: Shutdown,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

        // Collect TLS mappings from all site managers
        let all_tls_mappings: Vec<httpward_core::core::server_models::site_manager::TlsMapping> =
            self.instance
                .site_managers
                .iter()
                .flat_map(|sm| sm.tls_mappings().to_vec())
                .collect();
        let tls_enabled = !all_tls_mappings.is_empty();

        // Create HTTP service with dynamic middleware layers using LayerStackBuilder
        let error_handler = ErrorHandler::default();
        let base_service = service_fn(move |ctx: Context<()>, _req: Request<Body>| {
            let error_handler = error_handler.clone();
            async move {
                let response = error_handler
                    .create_error_response_with_code(StatusCode::NOT_FOUND)
                    .unwrap_or_else(|_| {
                        Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header("content-type", "text/plain")
                            .body(Body::from("Error template failed"))
                            .unwrap()
                    });

                Ok::<_, std::convert::Infallible>(response)
            }
        });

        // Use LayerStackBuilder for dynamic middleware composition
        let server_instance_arc = Arc::new(self.instance.clone());

        let http_svc = HttpServer::auto(exec.clone()).service(
            (
                ErrorHandlerLayer::new(),
                RequestEnricherLayer::new(server_instance_arc.clone()),
                DynamicModuleLoaderLayer::new(&server_instance_arc),
                ResponseEnricherLayer::new(),
                RouteLayer::new(),
            )
                .into_layer(base_service),
        );

        // Bind TCP listener and serve
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind server on {}: {}", addr, e))?;

        if tls_enabled {
            info!("TLS enabled for server on https://{}", display_addr);

            // Build TLS acceptor data with SNI support
            let tls_config = TlsConfigBuilder::new(all_tls_mappings);
            let tls_data = tls_config
                .build()
                .await
                .map_err(|e| format!("Failed to build TLS config: {}", e))?;

            // Serve connections with TLS
            listener
                .serve(
                    TlsAcceptorLayer::new(tls_data)
                        .with_store_client_hello(true)
                        .into_layer(http_svc),
                )
                .await;
        } else {
            warn!(
                "TLS not configured. Server running on http://{}",
                display_addr
            );

            // Serve connections without TLS
            listener.serve(http_svc).await;
        }

        Ok(())
    }
}
