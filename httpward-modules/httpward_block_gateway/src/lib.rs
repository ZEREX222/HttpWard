// httpward-modules/httpward_block_gateway/src/lib.rs
// HttpWard block gateway module

mod httpward_block_gateway_layer;
pub use httpward_block_gateway_layer::HttpWardBlockGatewayLayer;

// Name is taken automatically from CARGO_PKG_NAME ("httpward_block_gateway")
httpward_core::export_middleware_module!(HttpWardBlockGatewayLayer);
