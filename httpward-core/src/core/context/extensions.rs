/// Extensions map for storing arbitrary data in HttpWardContext during request lifetime
/// This allows middleware to share serialized objects without modifying context structure

use std::any::Any;
use std::sync::Arc;
use parking_lot::RwLock;
use std::collections::HashMap;

/// Thread-safe, cloneable storage for arbitrary data keyed by strings
/// Uses type erasure (Any) to support any Send + Sync type
#[derive(Clone)]
pub struct ExtensionsMap {
    inner: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,
}

impl ExtensionsMap {
    /// Create a new empty extensions map
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Insert a value into the extensions map
    ///
    /// # Arguments
    /// * `key` - The key to store the value under
    /// * `value` - The value to store (must be Send + Sync + 'static)
    ///
    /// # Example
    /// ```ignore
    /// ctx.extensions.insert("user_id", 12345u64);
    /// ctx.extensions.insert("claims", jwt_claims);
    /// ```
    pub fn insert<T: Any + Send + Sync + 'static>(&self, key: impl Into<String>, value: T) {
        self.inner.write().insert(key.into(), Arc::new(value));
    }

    /// Retrieve a value from the extensions map
    ///
    /// # Arguments
    /// * `key` - The key to retrieve
    ///
    /// # Returns
    /// * `Some(Arc<T>)` if the key exists and the type matches
    /// * `None` if the key doesn't exist or type mismatch occurs
    ///
    /// # Example
    /// ```ignore
    /// if let Some(user_id) = ctx.extensions.get::<u64>("user_id") {
    ///     println!("User ID: {}", user_id);
    /// }
    /// ```
    pub fn get<T: Any + Send + Sync + 'static>(&self, key: &str) -> Option<Arc<T>> {
        let inner = self.inner.read();
        let value = inner.get(key)?.clone();
        value.downcast::<T>().ok()
    }

    /// Check if a key exists in the extensions map
    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.read().contains_key(key)
    }

    /// Remove a value from the extensions map
    pub fn remove(&self, key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.inner.write().remove(key)
    }

    /// Clear all extensions
    pub fn clear(&self) {
        self.inner.write().clear();
    }

    /// Get the number of stored extensions
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Check if the extensions map is empty
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }
}

impl Default for ExtensionsMap {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ExtensionsMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionsMap")
            .field("count", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let ext = ExtensionsMap::new();

        // Test with u64
        ext.insert("user_id", 123u64);
        assert_eq!(ext.get::<u64>("user_id").map(|v| *v), Some(123u64));

        // Test with String
        ext.insert("session", "abc123".to_string());
        assert_eq!(ext.get::<String>("session").map(|v| (*v).clone()), Some("abc123".to_string()));
    }

    #[test]
    fn test_type_mismatch() {
        let ext = ExtensionsMap::new();
        ext.insert("value", 42u64);

        // Type mismatch should return None
        assert_eq!(ext.get::<String>("value"), None);
    }

    #[test]
    fn test_missing_key() {
        let ext = ExtensionsMap::new();
        assert_eq!(ext.get::<u64>("nonexistent"), None);
    }

    #[test]
    fn test_contains_key() {
        let ext = ExtensionsMap::new();
        ext.insert("key1", "value".to_string());

        assert!(ext.contains_key("key1"));
        assert!(!ext.contains_key("key2"));
    }

    #[test]
    fn test_remove() {
        let ext = ExtensionsMap::new();
        ext.insert("key", 42u64);

        assert!(ext.remove("key").is_some());
        assert!(ext.get::<u64>("key").is_none());
    }

    #[test]
    fn test_clone_shares_data() {
        let ext1 = ExtensionsMap::new();
        ext1.insert("shared", 999u64);

        let ext2 = ext1.clone();

        // Both should see the same data
        assert_eq!(ext2.get::<u64>("shared").map(|v| *v), Some(999u64));
    }

    #[test]
    fn test_multiple_types() {
        let ext = ExtensionsMap::new();

        ext.insert("int", 42u64);
        ext.insert("string", "hello".to_string());
        ext.insert("float", 3.14f64);

        assert_eq!(ext.len(), 3);
        assert_eq!(ext.get::<u64>("int").map(|v| *v), Some(42u64));
        assert_eq!(ext.get::<String>("string").map(|v| (*v).clone()), Some("hello".to_string()));
        assert_eq!(ext.get::<f64>("float").map(|v| *v), Some(3.14f64));
    }
}

