# HttpWardContext Extensions: Middleware Data Sharing Guide

## Overview

`HttpWardContext` now includes an **`extensions`** field (`ExtensionsMap`) that allows middleware to store and retrieve arbitrary data during request processing. This enables middleware to:

1. **Create and store** data (e.g., analysis results, parsed tokens)
2. **Share** that data with downstream middleware
3. **Retrieve** and use data from upstream middleware

## Architecture

### ExtensionsMap

A thread-safe, cloneable storage mechanism using type-erased values:

```rust
pub struct ExtensionsMap {
    inner: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,
}
```

**Key Features:**
- ✅ Thread-safe (RwLock + Arc)
- ✅ Type-safe access via generics
- ✅ Cloneable (cheap clone due to Arc)
- ✅ No runtime panics on type mismatch (returns Option)
- ✅ Works across async boundaries

## Usage Patterns

### 1. Basic Storage and Retrieval

```rust
use httpward_core::core::HttpWardContext;

async fn middleware_1(ctx: &HttpWardContext) {
    // Store arbitrary data
    ctx.extensions.insert("user_id", 12345u64);
    ctx.extensions.insert("token", "abc123xyz".to_string());
}

async fn middleware_2(ctx: &HttpWardContext) {
    // Retrieve data with type safety
    if let Some(user_id) = ctx.extensions.get::<u64>("user_id") {
        println!("User ID: {}", user_id);
    }
}
```

### 2. Storing Complex Types

```rust
#[derive(Clone, Debug)]
pub struct AnalysisResult {
    pub is_bot: bool,
    pub risk_score: u32,
}

// In middleware 1
let result = AnalysisResult {
    is_bot: false,
    risk_score: 25,
};
ctx.extensions.insert("analysis", result);

// In middleware 2
if let Some(result) = ctx.extensions.get::<AnalysisResult>("analysis") {
    if result.risk_score > 50 {
        println!("High risk request!");
    }
}
```

### 3. Multiple Middleware Coordination

```rust
// Middleware A: Extract fingerprint
async fn fingerprint_mw(ctx: &HttpWardContext) {
    let fp = calculate_fingerprint();
    ctx.extensions.insert("fingerprint", fp);
}

// Middleware B: Enrich with geolocation
async fn geo_mw(ctx: &HttpWardContext) {
    if let Some(fp) = ctx.extensions.get::<String>("fingerprint") {
        let location = lookup_location(&fp);
        ctx.extensions.insert("location", location);
    }
}

// Middleware C: Make security decision
async fn security_mw(ctx: &HttpWardContext) {
    match (
        ctx.extensions.get::<String>("fingerprint"),
        ctx.extensions.get::<Location>("location"),
    ) {
        (Some(fp), Some(loc)) => {
            // Make decision based on both signals
        }
        _ => {}
    }
}
```

## API Reference

### Insert Data

```rust
pub fn insert<T: Any + Send + Sync + 'static>(&self, key: impl Into<String>, value: T)
```

Stores a value with the given key. The value can be any type that implements `Send + Sync + 'static`.

**Example:**
```rust
ctx.extensions.insert("user_id", 42u64);
ctx.extensions.insert("claims", jwt_claims);
```

### Get Data

```rust
pub fn get<T: Any + Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>>
```

Retrieves a value by key. Returns `None` if:
- The key doesn't exist
- The stored type doesn't match the requested type `T`

**Example:**
```rust
if let Some(user_id) = ctx.extensions.get::<u64>("user_id") {
    println!("ID: {}", user_id);
}
```

### Contains Key

```rust
pub fn contains_key(&self, key: &str) -> bool
```

Checks if a key exists in the extensions map.

**Example:**
```rust
if ctx.extensions.contains_key("analysis_result") {
    // Process further
}
```

### Remove Data

```rust
pub fn remove(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>>
```

Removes and returns a value by key.

**Example:**
```rust
if let Some(data) = ctx.extensions.remove("temporary_data") {
    // Use and discard
}
```

### Clear All

```rust
pub fn clear(&self)
```

Removes all stored data.

**Example:**
```rust
ctx.extensions.clear();
```

### Length and Empty Check

```rust
pub fn len(&self) -> usize
pub fn is_empty(&self) -> bool
```

**Example:**
```rust
if ctx.extensions.is_empty() {
    println!("No extensions set");
}
```

## Best Practices

### 1. Use Meaningful Keys

```rust
// ✅ Good
ctx.extensions.insert("jwt_claims", claims);
ctx.extensions.insert("user_analysis", analysis);

// ❌ Avoid
ctx.extensions.insert("data1", claims);
ctx.extensions.insert("tmp", analysis);
```

### 2. Document Expected Keys

Create constants or enums for extension keys:

```rust
pub mod extension_keys {
    pub const JWT_CLAIMS: &str = "jwt_claims";
    pub const USER_ANALYSIS: &str = "user_analysis";
    pub const IP_GEOLOCATION: &str = "ip_geolocation";
}

// Usage
ctx.extensions.insert(extension_keys::JWT_CLAIMS, claims);
if let Some(claims) = ctx.extensions.get::<JwtClaims>(extension_keys::JWT_CLAIMS) {
    // ...
}
```

### 3. Handle Type Mismatches Gracefully

```rust
// ✅ Good: Handle both cases
match (
    ctx.extensions.get::<UserAnalysis>("analysis"),
    ctx.extensions.get::<JwtClaims>("claims"),
) {
    (Some(analysis), Some(claims)) => {
        // Use both
    }
    (Some(analysis), None) => {
        // Only analysis available
    }
    (None, Some(claims)) => {
        // Only claims available
    }
    (None, None) => {
        // No data
    }
}
```

### 4. Clone Values When Needed

Since `get()` returns `Arc<T>`, cloning the Arc is cheap:

```rust
let claims = ctx.extensions.get::<JwtClaims>("claims");

// For owned value, clone the inner data if needed:
if let Some(claims_arc) = claims {
    let claims_owned = (*claims_arc).clone();  // Clone inner data
    // Now you have owned JwtClaims
}
```

### 5. Middleware Ordering

Ensure middleware that produces data runs before middleware that consumes it:

```rust
// builder
    .add_layer(FingerprinterMiddleware)      // Produces "fingerprint"
    .add_layer(EnricherMiddleware)            // Consumes "fingerprint"
    .add_layer(SecurityDecisionMiddleware)    // Uses enriched data
```

## Performance Considerations

### Thread Safety
- Uses `parking_lot::RwLock` for better performance than `std::sync::RwLock`
- Multiple readers can access simultaneously
- Cloning the `ExtensionsMap` is cheap (just increments Arc refcount)

### Memory
- Data is stored in Arc, so each value is heap-allocated once
- Cheap cloning of maps between middleware
- No unnecessary copying

### Locking
- `insert()` and `remove()` take write locks (brief)
- `get()` takes read locks (non-blocking for concurrent readers)
- `clear()` takes write lock (expensive for large maps)

## Serialization Patterns

If you need to serialize/deserialize across boundaries:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerializableData {
    pub field1: String,
    pub field2: u64,
}

// Store
let data = SerializableData { field1: "test".into(), field2: 42 };
ctx.extensions.insert("serializable", data);

// Retrieve and serialize to JSON
if let Some(data) = ctx.extensions.get::<SerializableData>("serializable") {
    let json = serde_json::to_string(&*data)?;
    println!("Serialized: {}", json);
}
```

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_middleware_coordination() {
        let ctx = HttpWardContext::new(
            "127.0.0.1:8080".parse().unwrap(),
            Arc::new(server_instance),
        );
        
        // Simulate middleware 1
        ctx.extensions.insert("step1", "completed".to_string());
        
        // Simulate middleware 2
        assert_eq!(
            ctx.extensions.get::<String>("step1").map(|s| (*s).clone()),
            Some("completed".to_string())
        );
    }
}
```

## Examples

See `extensions_example.rs` for complete working examples of:
- Storing UserAnalysis results
- Storing JWT claims
- Multi-middleware coordination
- Type safety guarantees

## Troubleshooting

### Extension Not Found
```rust
// Check if it was actually stored
if !ctx.extensions.contains_key("my_data") {
    eprintln!("Data not stored!");
}
```

### Type Mismatch
```rust
// Verify the type you're using matches what was stored
// get::<WrongType>(...) will return None
// Use a debugger or logging to verify types
ctx.extensions.insert("value", 42u64);
assert_eq!(ctx.extensions.get::<u32>("value"), None);  // Type mismatch
assert_eq!(ctx.extensions.get::<u64>("value"), Some(Arc::new(42u64)));  // Correct
```

### Middleware Ordering
```rust
// Middleware B won't find data if Middleware A hasn't run yet
// Ensure correct ordering in builder:
builder
    .add_layer(ProducerMiddleware)   // Must come first
    .add_layer(ConsumerMiddleware)   // Uses data from producer
```

## Migration Guide

If you previously passed data through request headers or query parameters, you can now use extensions:

### Before
```rust
// Had to add custom headers
request.headers_mut().insert("X-Analysis", HeaderValue::from_static("..."));
// Other middleware had to parse headers
```

### After
```rust
// Store directly in context
ctx.extensions.insert("analysis", analysis_result);
// Other middleware can access directly
if let Some(analysis) = ctx.extensions.get::<AnalysisResult>("analysis") {
    // Use directly
}
```

