use httpward_core::middleware::{Middleware, MiddlewareResult};
use httpward_core::context::RequestContext;
use hyper::{Request, Response, body::Incoming};

pub struct MiddlewarePipeline {
    layers: Vec<Box<dyn Middleware>>,
}

impl MiddlewarePipeline {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn add(&mut self, layer: Box<dyn Middleware>) {
        self.layers.push(layer);
    }

    pub fn execute(
        &self,
        mut req: Request<Incoming>,
        ctx: &mut RequestContext
    ) -> MiddlewareResult {
        for layer in &self.layers {
            match layer.handle(req, ctx) {
                MiddlewareResult::Next(next_req) => req = next_req,
                MiddlewareResult::Respond(res) => return MiddlewareResult::Respond(res),
            }
        }
        MiddlewareResult::Next(req)
    }
}
