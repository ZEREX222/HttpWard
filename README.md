![hw.png](resources/hw.png)
# HttpWard
HttpWard is a lightweight, high-performance L7 reverse proxy written in Rust, focused on strong security (WAF, rate limiting, DDoS mitigation), intelligent caching, flexible virtual host routing, and extremely low resource usage.

## Features

- **Middleware Pipeline System**: Clean and flexible middleware chaining
- **Request Enrichment**: Automatic context enrichment with client info and content type
- **Structured Logging**: Comprehensive request/response logging
- **Type Safety**: Compile-time guaranteed middleware combinations
- **Easy Integration**: Multiple ways to build middleware pipelines

## Quick Start

### Using the Middleware Pipeline

```rust
use httpward_core::middleware::PrebuiltPipelines;
use rama::service::service_fn;

// Standard pipeline: Enricher -> Log
let service = PrebuiltPipelines::standard(service_fn(handler));

// Or build custom pipelines
use httpward_core::middleware::MiddlewarePipe;
let service = MiddlewarePipe::new()
    .add_enricher()
    .add_log()
    .build(service_fn(handler));
```

### Migration from Nested Layers

**Before:**
```rust
let service = EnricherLayer::new().layer(
    LogLayer::new().layer(
        service_fn(handler)
    )
);
```

**After:**
```rust
let service = PrebuiltPipelines::standard(service_fn(handler));
```

## Available Middleware

- **EnricherLayer**: Enriches requests with client address and content type
- **LogLayer**: Provides structured request/response logging

## Documentation

See [docs/middleware-pipeline.md](docs/middleware-pipeline.md) for detailed usage guide.
