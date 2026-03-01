use tracing::{error, info};
// server/manager.rs
use crate::server::http_server::HttpServer;

pub struct ServerManager;

impl ServerManager {
    /// Spawns all servers into the Tokio runtime and waits for them
    pub async fn start_all<'a>(servers: Vec<HttpServer>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut handles = vec![];

        for server in servers {
            let host = &server.instance.bind.host;
            let port = server.instance.bind.port;
            let addr_str = format!("{}:{}", host, port);
            
            info!("📡 Starting HttpServer on {}", addr_str);

            // We move the server into a spawned task
            let handle = tokio::spawn(async move {
                if let Err(e) = server.run().await {
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
