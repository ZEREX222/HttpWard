use std::collections::HashMap;
use std::sync::{Mutex, Arc};
use std::path::Path;
use tracing::{info, error, warn};
use libloading::Library;
use super::middleware_module_instance::MiddlewareModuleInstance;
use super::middleware_module_load_manager::LoadedModule;

/// Global storage for loaded middleware modules
static GLOBAL_MODULE_STORAGE: Mutex<Option<GlobalModuleStorage>> = Mutex::new(None);

/// Global module storage that holds all loaded middleware instances
#[derive(Debug)]
pub struct GlobalModuleStorage {
    modules: HashMap<String, LoadedModule>,
}

impl GlobalModuleStorage {
    /// Create new global storage
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }
    
    /// Add a module to storage
    pub fn add_module(&mut self, module: LoadedModule) {
        info!(target: "global_module_storage", "Adding module '{}' to global storage", module.name);
        self.modules.insert(module.name.clone(), module);
    }
    
    /// Get middleware instance by name
    pub fn get_middleware_instance(&self, name: &str) -> Option<MiddlewareModuleInstance> {
        if let Some(module) = self.modules.get(name) {
            info!(target: "global_module_storage", "Creating middleware instance for '{}'", name);
            // Safety: Module loading is unsafe by nature
            unsafe {
                // Reload the library from the stored path to get a fresh instance
                match Library::new(&module.path) {
                    Ok(library) => {
                        match MiddlewareModuleInstance::create_middleware_instance(Arc::new(library)) {
                            Ok(instance) => {
                                info!(target: "global_module_storage", "Successfully created middleware instance for '{}'", name);
                                Some(instance)
                            }
                            Err(e) => {
                                error!(target: "global_module_storage", "Failed to create middleware instance '{}': {}", name, e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        error!(target: "global_module_storage", "Failed to reload library for '{}': {}", name, e);
                        None
                    }
                }
            }
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
pub fn get_middleware_instance(name: &str) -> Option<MiddlewareModuleInstance> {
    if let Ok(storage) = GLOBAL_MODULE_STORAGE.lock() {
        if let Some(ref global_storage) = *storage {
            global_storage.get_middleware_instance(name)
        } else {
            warn!(target: "global_module_storage", "Global module storage not initialized");
            None
        }
    } else {
        error!(target: "global_module_storage", "Failed to acquire global module storage lock");
        None
    }
}

/// Initialize global module storage
pub fn initialize_global_storage() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut guard = GLOBAL_MODULE_STORAGE.lock()
        .map_err(|e| format!("Failed to acquire global storage lock: {}", e))?;
    
    if guard.is_some() {
        warn!(target: "global_module_storage", "Global module storage already initialized");
        return Ok(());
    }
    
    *guard = Some(GlobalModuleStorage::new());
    info!(target: "global_module_storage", "Global module storage initialized");
    Ok(())
}

/// Get access to global storage (for internal operations)
pub fn with_global_storage<F, R>(f: F) -> Result<R, Box<dyn std::error::Error + Send + Sync>>
where
    F: FnOnce(&mut GlobalModuleStorage) -> R,
{
    let mut guard = GLOBAL_MODULE_STORAGE.lock()
        .map_err(|e| format!("Failed to acquire global storage lock: {}", e))?;
    
    if let Some(ref mut global_storage) = *guard {
        Ok(f(global_storage))
    } else {
        Err("Global module storage not initialized".into())
    }
}

/// Check if global storage is initialized
pub fn is_global_storage_initialized() -> bool {
    if let Ok(guard) = GLOBAL_MODULE_STORAGE.lock() {
        guard.is_some()
    } else {
        false
    }
}

/// Get global storage statistics
pub fn get_global_storage_stats() -> Option<(usize, Vec<String>)> {
    if let Ok(guard) = GLOBAL_MODULE_STORAGE.lock() {
        if let Some(ref global_storage) = *guard {
            Some((global_storage.module_count(), global_storage.module_names()))
        } else {
            None
        }
    } else {
        None
    }
}
