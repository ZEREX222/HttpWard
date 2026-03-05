use tracing::{error, info};
use rama::graceful::Shutdown;
use crate::server::http_server::HttpWardServer;

pub struct HttpWardServerManager;

impl HttpWardServerManager {
    /// Spawns all servers into the Tokio runtime and waits for them
    pub async fn start_all(servers: Vec<HttpWardServer>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut handles = vec![];

        for server in servers {
            let host = server.instance.bind.host.clone();
            let port = server.instance.bind.port;
            let addr_str = format!("{}:{}", host, port);
            
            info!("📡 Starting HttpWardServer on {}", addr_str);

            // Create a shutdown channel for this server
            let (_shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
            let shutdown = Shutdown::new(shutdown_rx);
            
            // We move the server into a spawned task
            let handle = tokio::spawn(async move {
                if let Err(e) = server.run(shutdown).await {
                    error!("🔥 Server at {} stopped with error: {}", addr_str, e);
                }
            });
            handles.push(handle);
        }

        // Keep the main thread alive while servers are running
        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }
}
