# Middleware Pipeline Usage Guide

This guide shows how to use the new middleware pipeline system in HttpWard.

## Overview

The middleware pipeline provides a clean way to chain multiple middleware layers without nested `.layer()` calls.

## Available Options

### 1. PrebuiltPipelines (Recommended)

Use predefined pipelines for common use cases:

```rust
use httpward_core::middleware::PrebuiltPipelines;
use rama::service::service_fn;

// Standard pipeline: Enricher -> Log
let service = PrebuiltPipelines::standard(base_service);

// Logging only
let service = PrebuiltPipelines::log_only(base_service);

// Enrichment only  
let service = PrebuiltPipelines::enrich_only(base_service);
```

### 2. MiddlewarePipe Builder

Build custom pipelines step by step:

```rust
use httpward_core::middleware::MiddlewarePipe;

// Enricher -> Log
let service = MiddlewarePipe::new()
    .add_enricher()
    .add_log()
    .build(base_service);

// Log -> Enricher (reversed order)
let service = MiddlewarePipe::new()
    .add_log()
    .add_enricher()
    .build(base_service);

// Single layer
let service = MiddlewarePipe::new()
    .add_enricher()
    .build(base_service);
```

### 3. Macro Approach

Use the `build_pipeline!` macro for maximum flexibility:

```rust
use httpward_core::build_pipeline;
use httpward_core::middleware::{EnricherLayer, LogLayer};

// Custom order with multiple layers
let service = build_pipeline!(
    base_service,
    EnricherLayer::new(),
    LogLayer::new(),
    // Add more layers here as needed
);
```

## Migration from Nested Layers

### Before (Old way):
```rust
let service = EnricherLayer::new().layer(
    LogLayer::new().layer(
        service_fn(handler)
    )
);
```

### After (New way):
```rust
let service = PrebuiltPipelines::standard(service_fn(handler));
```

Or:
```rust
let service = MiddlewarePipe::new()
    .add_enricher()
    .add_log()
    .build(service_fn(handler));
```

## Layer Order

The order of layers matters:

- **Enricher -> Log**: Enriches request context first, then logs the enriched request
- **Log -> Enricher**: Logs the raw request first, then enriches it

For most use cases, **Enricher -> Log** is recommended.

## Complete Example

```rust
use httpward_core::middleware::{PrebuiltPipelines, HttpWardContext};
use rama::{service::service_fn, Context};
use rama::http::{Request, Response, Body, StatusCode};

async fn handler(ctx: Context<()>, req: Request<Body>) -> Result<Response<Body>, std::convert::Infallible> {
    // Access enriched context
    if let Some(http_ctx) = ctx.get::<HttpWardContext>() {
        println!("Client: {:?}", http_ctx.client_addr);
        println!("Content Type: {:?}", http_ctx.content_type);
    }
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::from("Hello World"))
        .unwrap())
}

// Create service with middleware pipeline
let service = PrebuiltPipelines::standard(service_fn(handler));
```

## Benefits

1. **Cleaner Code**: No nested `.layer()` calls
2. **Type Safety**: Compile-time guarantee of correct layer combinations
3. **Flexibility**: Multiple ways to build pipelines
4. **Readability**: Clear intent and structure
5. **Maintainability**: Easy to add/remove/reorder layers
