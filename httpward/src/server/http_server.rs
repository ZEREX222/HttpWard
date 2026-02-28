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
use httpward_core::middleware::{Middleware, MiddlewareFuture, Next, RequestContext};

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

            // We clone the Arc of the entire pipeline
            let pipeline = Arc::clone(&self.pipeline);

            tokio::task::spawn(async move {
                let service = service_fn(move |req| {
                    let mut ctx = RequestContext::new(client_addr);
                    let pipe = Arc::clone(&pipeline);

                    async move {
                        // We start the recursive chain execution
                        execute_pipeline(0, pipe, req, &mut ctx).await
                    }
                });

                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    error!("[Server Error] {}: {:?}", client_addr, err);
                }
            });
        }
    }
}

/// This function handles the recursive "wrapping" of middlewares.
fn execute_pipeline<'a>(
    index: usize,
    pipeline: Arc<Vec<Box<dyn Middleware>>>,
    req: hyper::Request<hyper::body::Incoming>,
    ctx: &'a mut RequestContext,
) -> MiddlewareFuture<'a> {
    Box::pin(async move {
        if index < pipeline.len() {
            // Get the current middleware
            let middleware = &pipeline[index];

            // Define what "next" does: it calls execute_pipeline for the NEXT index
            let next_pipeline = Arc::clone(&pipeline);
            let next = Box::new(move |next_req, next_ctx| {
                execute_pipeline(index + 1, next_pipeline, next_req, next_ctx)
            });

            // Execute the current middleware
            middleware.handle(req, ctx, next).await
        } else {
            // --- FINAL STEP: No more middlewares, contact the Backend ---
            // In the future, this is where your Reverse Proxy Client logic lives.
            //let backend_body = Either::Right(req.into_body());
            // For now, we just echo back as a placeholder:
            Ok(hyper::Response::builder()
                .status(200)
                .body(Either::Left(Full::new(Bytes::from("Backend Reached"))))
                .unwrap())
        }
    })
}
