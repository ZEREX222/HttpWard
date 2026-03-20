use tracing::{error, info};
use rama::graceful::Shutdown;
use crate::server::http_server::HttpWardServer;

pub struct HttpWardServerManager;

impl HttpWardServerManager {
    /// Spawns all servers into the Tokio runtime and waits for them
    pub async fn start_all(servers: Vec<HttpWardServer>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut handles = vec![];
        let mut shutdown_txs = vec![];

        // First pass: create all shutdown channels and spawn servers
        for server in servers {
            let host = server.instance.bind.host.clone();
            let port = server.instance.bind.port;
            let addr_str = format!("{}:{}", host, port);
            
            info!("📡 Starting HttpWardServer on {}", addr_str);

            // Create a shutdown channel for this server
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
            let shutdown = Shutdown::new(shutdown_rx);
            
            // Store sender to keep it alive for the duration of the server
            shutdown_txs.push(shutdown_tx);

            // We move the server into a spawned task
            let handle = tokio::spawn(async move {
                if let Err(e) = server.run(shutdown).await {
                    error!("🔥 Server at {} stopped with error: {}", addr_str, e);
                }
            });
            handles.push(handle);
        }

        info!("✅ All {} servers started successfully", handles.len());

        // Keep the main thread alive while servers are running
        for handle in handles {
            let _ = handle.await;
        }

        // Shutdown senders are dropped here, signaling graceful shutdown to all servers
        drop(shutdown_txs);

        Ok(())
    }
}
