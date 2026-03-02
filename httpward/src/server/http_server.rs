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

        if tls_enabled {
            info!("TLS enabled for server on https://{}", display_addr);
        } else {
            warn!("TLS not configured. Server running on http://{}", display_addr);
        }

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

                // Serve connections
                listener
                    .serve(http_svc)
                    .await;

                Ok(())
            }
            Err(e) => {
                error!("Failed to bind server on {}: {}", addr, e);
                Err(format!("Bind error: {}", e).into())
            }
        }
    }
}
