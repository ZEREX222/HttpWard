# HttpWardContext Extensions - Migration & Best Practices Guide

## 🎯 Quick Start

### Basic Usage

```rust
// In your middleware:
if let Some(http_ctx) = ctx.get::<HttpWardContext>() {
    // Store data
    http_ctx.extensions.insert("my_data", MyData { ... });
    
    // Retrieve data
    if let Some(data) = http_ctx.extensions.get::<MyData>("my_data") {
        println!("Data: {:?}", data);
    }
}
```

---

## 📋 Pattern Guide

### Pattern 1: Multi-Step Analysis

Store intermediate results as each middleware enriches the context:

```rust
// Middleware A: Extract fingerprint
struct FingerprintMw;
impl HttpWardMiddleware for FingerprintMw {
    async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
        if let Some(http_ctx) = ctx.get::<HttpWardContext>() {
            let fp = extract_fingerprint(&req);
            http_ctx.extensions.insert("fingerprint", fp);
        }
        next.run(ctx, req).await
    }
}

// Middleware B: Enrich with analysis
struct AnalysisMw;
impl HttpWardMiddleware for AnalysisMw {
    async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
        if let Some(http_ctx) = ctx.get::<HttpWardContext>() {
            // Read data from previous middleware
            if let Some(fp) = http_ctx.extensions.get::<Fingerprint>("fingerprint") {
                let analysis = perform_analysis(&fp);
                http_ctx.extensions.insert("analysis", analysis);
            }
        }
        next.run(ctx, req).await
    }
}

// Middleware C: Make decision
struct DecisionMw;
impl HttpWardMiddleware for DecisionMw {
    async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
        if let Some(http_ctx) = ctx.get::<HttpWardContext>() {
            match (
                http_ctx.extensions.get::<Fingerprint>("fingerprint"),
                http_ctx.extensions.get::<Analysis>("analysis"),
            ) {
                (Some(fp), Some(ana)) => {
                    let decision = make_decision(fp, ana);
                    http_ctx.extensions.insert("decision", decision);
                }
                _ => {}
            }
        }
        next.run(ctx, req).await
    }
}
```

### Pattern 2: Caching Results

Store expensive computations:

```rust
struct CachingMw;
impl HttpWardMiddleware for CachingMw {
    async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
        if let Some(http_ctx) = ctx.get::<HttpWardContext>() {
            // Check cache
            if http_ctx.extensions.contains_key("cached_result") {
                // Use cached version
                if let Some(result) = http_ctx.extensions.get::<CachedResult>("cached_result") {
                    return Ok(create_response(&result));
                }
            }
            
            // Compute and cache
            let result = expensive_computation();
            http_ctx.extensions.insert("cached_result", result.clone());
        }
        next.run(ctx, req).await
    }
}
```

### Pattern 3: Conditional Middleware

Skip middleware based on data from previous middleware:

```rust
struct ConditionalMw;
impl HttpWardMiddleware for ConditionalMw {
    async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
        if let Some(http_ctx) = ctx.get::<HttpWardContext>() {
            if let Some(analysis) = http_ctx.extensions.get::<Analysis>("analysis") {
                if analysis.risk_score > 100 {
                    // Skip expensive processing for low-risk requests
                    return next.run(ctx, req).await;
                }
            }
        }
        
        // Only executed if condition not met
        expensive_security_check(&req);
        next.run(ctx, req).await
    }
}
```

### Pattern 4: Data Aggregation

Collect data from multiple sources:

```rust
struct AggregatorMw;
impl HttpWardMiddleware for AggregatorMw {
    async fn handle(&self, ctx: Context<()>, req: Request<Body>, next: Next<'_>) -> Result<Response<Body>, BoxError> {
        if let Some(http_ctx) = ctx.get::<HttpWardContext>() {
            let mut report = SecurityReport::new();
            
            // Aggregate all available data
            if let Some(fp) = http_ctx.extensions.get::<Fingerprint>("fingerprint") {
                report.add_fingerprint(&fp);
            }
            if let Some(ana) = http_ctx.extensions.get::<Analysis>("analysis") {
                report.add_analysis(&ana);
            }
            if let Some(loc) = http_ctx.extensions.get::<Location>("location") {
                report.add_location(&loc);
            }
            
            http_ctx.extensions.insert("security_report", report);
        }
        next.run(ctx, req).await
    }
}
```

---

## 🔑 Key Naming Conventions

### Create a Constants Module

```rust
// src/core/extension_keys.rs

pub mod extension_keys {
    // Fingerprinting
    pub const CLIENT_FINGERPRINT: &str = "client_fingerprint";
    pub const JA4_FINGERPRINT: &str = "ja4_fingerprint";
    pub const HEADER_FINGERPRINT: &str = "header_fingerprint";
    
    // Analysis
    pub const BEHAVIORAL_ANALYSIS: &str = "behavioral_analysis";
    pub const RISK_ASSESSMENT: &str = "risk_assessment";
    pub const BOT_DETECTION: &str = "bot_detection";
    
    // Enrichment
    pub const IP_GEOLOCATION: &str = "ip_geolocation";
    pub const ASN_INFO: &str = "asn_info";
    pub const WHOIS_DATA: &str = "whois_data";
    
    // Authentication
    pub const JWT_CLAIMS: &str = "jwt_claims";
    pub const USER_IDENTITY: &str = "user_identity";
    pub const API_KEY_DATA: &str = "api_key_data";
    
    // Metadata
    pub const REQUEST_TIMING: &str = "request_timing";
    pub const PROCESSING_METADATA: &str = "processing_metadata";
}
```

Then use consistently:

```rust
// Instead of magic strings
// ❌ ctx.extensions.insert("fp", fingerprint);
// ✅ ctx.extensions.insert(extension_keys::CLIENT_FINGERPRINT, fingerprint);

http_ctx.extensions.insert(extension_keys::CLIENT_FINGERPRINT, fingerprint);

if let Some(fp) = http_ctx.extensions.get::<Fingerprint>(extension_keys::CLIENT_FINGERPRINT) {
    // ...
}
```

---

## 🛡️ Type Safety Best Practices

### Use Type-Safe Wrappers

```rust
// Define wrapper types for clarity
#[derive(Clone, Debug)]
pub struct Fingerprint(pub String);

#[derive(Clone, Debug)]
pub struct AnalysisResult(pub u32);

// Now usage is type-safe and clear
http_ctx.extensions.insert(ext_keys::CLIENT_FINGERPRINT, Fingerprint(fp));

if let Some(result) = http_ctx.extensions.get::<Fingerprint>(ext_keys::CLIENT_FINGERPRINT) {
    let fp = &result.0;
    // ...
}
```

### Graceful Error Handling

```rust
// ❌ Panic if data missing
let data = http_ctx.extensions.get::<MyData>("key").unwrap();

// ✅ Handle gracefully
match http_ctx.extensions.get::<MyData>("key") {
    Some(data) => {
        // Process data
    }
    None => {
        // Log warning or use default
        warn!("Expected data not found, using defaults");
    }
}

// ✅ Use unwrap_or for defaults
let data = http_ctx.extensions
    .get::<MyData>("key")
    .map(|arc| (*arc).clone())
    .unwrap_or_else(|| MyData::default());
```

---

## 📊 Serialization for Logging

```rust
use serde_json;

// Serialize extensions for logging
if let Some(http_ctx) = ctx.get::<HttpWardContext>() {
    let mut ext_map = std::collections::BTreeMap::new();
    
    if let Some(fp) = http_ctx.extensions.get::<Fingerprint>(ext_keys::CLIENT_FINGERPRINT) {
        ext_map.insert("fingerprint", serde_json::to_value(&fp).ok());
    }
    
    if let Some(ana) = http_ctx.extensions.get::<Analysis>(ext_keys::BEHAVIORAL_ANALYSIS) {
        ext_map.insert("analysis", serde_json::to_value(&ana).ok());
    }
    
    info!("Request context: {:?}", ext_map);
}
```

---

## ⚡ Performance Tips

### 1. Minimize Lock Contention

```rust
// ❌ Multiple separate locks
let fp = http_ctx.extensions.get::<Fingerprint>("fingerprint");  // Lock 1
let ana = http_ctx.extensions.get::<Analysis>("analysis");        // Lock 2
let loc = http_ctx.extensions.get::<Location>("location");        // Lock 3

// ✅ Load data once (if possible)
let fp = http_ctx.extensions.get::<Fingerprint>("fingerprint");   // Lock 1
let (ana, loc) = (
    http_ctx.extensions.get::<Analysis>("analysis"),      // Lock 2
    http_ctx.extensions.get::<Location>("location"),      // Lock 3
);
// Multiple locks are fine in practice - parking_lot is very fast
```

### 2. Use Arc Efficiently

```rust
// Remember: get() returns Arc<T>, not T
// No need to clone unless you store it longer-term

// ✅ Use reference without cloning
if let Some(data) = http_ctx.extensions.get::<MyData>("key") {
    process(&data);  // data is Arc<MyData>
}

// ❌ Unnecessary clone
if let Some(data) = http_ctx.extensions.get::<MyData>("key") {
    let cloned = (*data).clone();  // Only if you need owned copy
}
```

### 3. Lazy Initialization

```rust
// ❌ Always compute
let data = expensive_computation();
http_ctx.extensions.insert("key", data);

// ✅ Compute only if needed
if !http_ctx.extensions.contains_key("key") {
    let data = expensive_computation();
    http_ctx.extensions.insert("key", data);
}
```

---

## 🧪 Testing Middleware with Extensions

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::Arc;

    #[test]
    fn test_middleware_with_extensions() {
        // Create test context
        let addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
        let server_instance = Arc::new(create_test_server());
        let ctx = HttpWardContext::new(addr, server_instance);
        
        // Test data flow
        ctx.extensions.insert("fingerprint", Fingerprint("test_fp".to_string()));
        
        // Verify stored
        assert!(ctx.extensions.contains_key("fingerprint"));
        
        // Verify retrieval
        assert_eq!(
            ctx.extensions.get::<Fingerprint>("fingerprint"),
            Some(Arc::new(Fingerprint("test_fp".to_string())))
        );
    }

    #[tokio::test]
    async fn test_async_middleware() {
        // Test async middleware behavior
        let result = FingerprintMw.handle(
            Context::default(),
            create_test_request(),
            create_mock_next(),
        ).await;
        
        assert!(result.is_ok());
    }
}
```

---

## 🔄 Migration from Old Patterns

### Before: Using Headers

```rust
// ❌ Old way: Store in headers
req.headers_mut().insert(
    "X-Analysis-Result",
    HeaderValue::from_static("suspicious"),
);

// Other middleware had to parse headers
if let Ok(val) = req.headers().get("X-Analysis-Result").and_then(|v| v.to_str()) {
    if val == "suspicious" {
        // ...
    }
}
```

### After: Using Extensions

```rust
// ✅ New way: Store in extensions
#[derive(Clone)]
struct Analysis { is_suspicious: bool }

http_ctx.extensions.insert(ext_keys::BEHAVIORAL_ANALYSIS, 
    Analysis { is_suspicious: true });

// Other middleware can access directly with type safety
if let Some(analysis) = http_ctx.extensions.get::<Analysis>(ext_keys::BEHAVIORAL_ANALYSIS) {
    if analysis.is_suspicious {
        // ...
    }
}
```

---

## 📈 Scaling Considerations

### Large Numbers of Extensions

If you find yourself storing many extensions, consider:

```rust
// Define a container struct
#[derive(Clone)]
pub struct RequestContext {
    pub fingerprint: Option<Fingerprint>,
    pub analysis: Option<Analysis>,
    pub location: Option<Location>,
    pub auth: Option<AuthInfo>,
}

// Store once as a single extension
http_ctx.extensions.insert(ext_keys::REQUEST_CONTEXT, 
    RequestContext {
        fingerprint: Some(...),
        analysis: Some(...),
        location: Some(...),
        auth: Some(...),
    });

// Access everything at once
if let Some(req_ctx) = http_ctx.extensions.get::<RequestContext>(ext_keys::REQUEST_CONTEXT) {
    // All data available
}
```

---

## 🚀 Complete Example: Security Pipeline

See `extensions_practical_example.rs` for a complete, runnable example with:
- ✅ Multiple middleware types
- ✅ Data enrichment patterns
- ✅ Security decision making
- ✅ Error handling
- ✅ Test setup

---

## 📚 Related Documentation

- See `extensions-guide.md` for API reference
- See `extensions_example.rs` for simple examples
- See `extensions_practical_example.rs` for production patterns
- See `EXTENSIONS_IMPLEMENTATION.md` for architecture details

---

## ✅ Checklist: Using Extensions in Production

- [ ] Define extension key constants
- [ ] Create data structures with `#[derive(Clone, Debug)]`
- [ ] Document data flow between middleware
- [ ] Add tests for middleware coordination
- [ ] Add logging for debugging
- [ ] Handle missing extensions gracefully
- [ ] Consider serialization for caching/logging
- [ ] Profile for any lock contention issues
- [ ] Document middleware ordering requirements
- [ ] Add examples to codebase

