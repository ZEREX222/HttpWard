use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tracing::{info, warn};
use libloading::Library;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;

/// Global storage for loaded middleware modules
/// Thread-safe initialization with OnceLock + Mutex
static GLOBAL_MODULE_STORAGE: OnceLock<Mutex<GlobalModuleStorage>> = OnceLock::new();

/// ModuleRecord: single library + single middleware instance
pub struct ModuleRecord {
    pub name: String,
    pub library: Option<Arc<Library>>,
    pub instance: Arc<dyn HttpWardMiddleware + Send + Sync>,
    pub path: std::path::PathBuf,
}

/// Global module storage that holds all loaded middleware instances
pub struct GlobalModuleStorage {
    modules: HashMap<String, ModuleRecord>,
}

impl GlobalModuleStorage {
    /// Create new global storage
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }
    
    /// Add a module to storage
    pub fn add_module(&mut self, module: ModuleRecord) {
        info!(target: "global_module_storage", "Adding module '{}' to global storage", module.name);
        self.modules.insert(module.name.clone(), module);
    }
    
    /// Get middleware instance by name
    pub fn get_middleware_instance(&self, name: &str) -> Option<Arc<dyn HttpWardMiddleware + Send + Sync>> {
        if let Some(record) = self.modules.get(name) {
            info!(target: "global_module_storage", "Returning middleware instance for '{}'", name);
            Some(record.instance.clone())
        } else {
            warn!(target: "global_module_storage", "Module '{}' not found in global storage", name);
            None
        }
    }
    
    /// Check if module exists
    pub fn has_module(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }
    
    /// Get all module names
    pub fn module_names(&self) -> Vec<String> {
        self.modules.keys().cloned().collect()
    }
    
    /// Get module count
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }
}

/// Get global middleware instance by name
pub fn get_middleware_instance(name: &str) -> Option<Arc<dyn HttpWardMiddleware + Send + Sync>> {
    let storage = GLOBAL_MODULE_STORAGE.get()?;
    let guard = storage.lock().ok()?;
    guard.get_middleware_instance(name)
}

/// Initialize global module storage
pub fn initialize_global_storage() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let was_already_initialized = GLOBAL_MODULE_STORAGE.get().is_some();
    let _storage = GLOBAL_MODULE_STORAGE.get_or_init(|| Mutex::new(GlobalModuleStorage::new()));
    
    if !was_already_initialized {
        info!(target: "global_module_storage", "Global module storage initialized");
    }
    
    Ok(())
}

/// Get access to global storage (for internal operations)
pub fn with_global_storage<F, R>(f: F) -> Result<R, Box<dyn std::error::Error + Send + Sync>>
where
    F: FnOnce(&mut GlobalModuleStorage) -> R,
{
    let storage = GLOBAL_MODULE_STORAGE.get_or_init(|| Mutex::new(GlobalModuleStorage::new()));
    let mut guard = storage.lock()
        .map_err(|e| format!("Failed to acquire global storage lock: {}", e))?;
    Ok(f(&mut guard))
}

/// Check if global storage is initialized
pub fn is_global_storage_initialized() -> bool {
    GLOBAL_MODULE_STORAGE.get().is_some()
}

/// Get global storage statistics
pub fn get_global_storage_stats() -> Option<(usize, Vec<String>)> {
    let storage = GLOBAL_MODULE_STORAGE.get()?;
    let guard = storage.lock().ok()?;
    Some((guard.module_count(), guard.module_names()))
}
