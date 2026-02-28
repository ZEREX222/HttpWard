// server/http_server.rs
use tokio::net::{TcpListener};
use std::net::SocketAddr;
use std::sync::Arc;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use http_body_util::{Full, Either};
use hyper::body::Bytes;
use tracing::error;
use httpward_core::middleware::{Middleware, MiddlewareResult, RequestContext};

pub struct HttpServer {
    pub addr: SocketAddr,
    pub pipeline: Arc<Vec<Box<dyn Middleware>>>,
}

impl HttpServer {
    pub fn new(host: &str, port: u16, pipeline: Vec<Box<dyn Middleware>>) -> Self {
        let addr = format!("{}:{}", host, port).parse().expect("Invalid SocketAddr");
        Self {
            addr,
            pipeline: Arc::new(pipeline),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(self.addr).await?;

        loop {
            let (stream, client_addr) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let pipeline = Arc::clone(&self.pipeline);

            tokio::task::spawn(async move {
                let service = service_fn(move |req| {
                    let mut ctx = RequestContext::new(client_addr);
                    let pipe = Arc::clone(&pipeline);

                    async move {
                        let mut current_req = req;

                        // 1. Execute Middleware Pipeline
                        for middleware in pipe.iter() {
                            match middleware.handle(current_req, &mut ctx) {
                                MiddlewareResult::Next(next_req) => {
                                    current_req = next_req;
                                }
                                MiddlewareResult::Respond(res) => {
                                    // Wrap middleware response body in 'Left'
                                    let (parts, body) = res.into_parts();
                                    let full_res = hyper::Response::from_parts(parts, Either::Left(Full::new(body)));
                                    return Ok::<_, hyper::Error>(full_res);
                                }
                            }
                        }

                        // 2. Final Step: Proxy to Backend (Placeholder)
                        // In a real proxy, 'Right' would be the stream from the backend
                        let backend_body = Either::Right(Full::new(Bytes::from("Passed to Backend")));
                        Ok::<_, hyper::Error>(hyper::Response::new(backend_body))
                    }
                });

                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    error!("[Server Error] {}: {:?}", client_addr, err);
                }
            });
        }
    }
}
