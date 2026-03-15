use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use libloading::Library;
use httpward_core::core::server_models::{ServerInstance, SiteManager};
use httpward_core::config::strategy::MiddlewareConfig;
use tracing::{info, error, warn};
use super::middleware_module_instance::MiddlewareModuleInstance;
use super::middleware_global_module_storage::{initialize_global_storage, with_global_storage};

/// Manager for dynamic loading of middleware modules based on strategy configuration
#[derive(Debug)]
pub struct MiddlewareModuleLoadManager {
    /// Loaded middleware libraries
    loaded_modules: Vec<LoadedModule>,
    /// Modules directory path
    modules_dir: PathBuf,
}

/// Information about a loaded middleware module
#[derive(Debug)]
pub struct LoadedModule {
    /// Module name
    pub name: String,
    /// Loaded library (wrapped in Arc for shared ownership)
    pub library: Arc<Library>,
    /// File path
    pub path: PathBuf,
}

impl MiddlewareModuleLoadManager {
    /// Create new module load manager from multiple server instances
    pub fn from_server_instances(server_instances: &[ServerInstance]) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let modules_dir = Path::new("modules").to_path_buf();
        
        // Initialize global storage if not already done
        initialize_global_storage()?;
        
        // Extract unique middleware names from all server instances
        let middleware_names = Self::extract_middleware_names_from_instances(server_instances);
        
        info!(target: "module_load_manager", "Found {} unique middleware names from {} server instances: {:?}", 
              middleware_names.len(), server_instances.len(), middleware_names);
        
        let mut manager = Self {
            loaded_modules: Vec::new(),
            modules_dir,
        };
        
        // Load modules for each middleware name and add to global storage
        for name in middleware_names {
            if let Err(e) = manager.load_module_by_name_and_add_to_global(&name) {
                error!(target: "module_load_manager",
                       "Failed to load module '{}': {}", name, e);
                return Err(e);
            }
        }
        
        info!(target: "module_load_manager", 
              "Successfully loaded {} middleware modules", manager.loaded_modules.len());
        
        Ok(manager)
    }
    
    /// Create new module load manager from single server instance (for backward compatibility)
    pub fn from_server_instance(server_instance: &ServerInstance) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::from_server_instances(&[server_instance.clone()])
    }
    
    /// Extract unique middleware names from multiple server instances
    fn extract_middleware_names_from_instances(server_instances: &[ServerInstance]) -> Vec<String> {
        let mut middleware_names = HashSet::new();
        
        for (idx, server_instance) in server_instances.iter().enumerate() {
            info!(target: "module_load_manager", "Processing server instance {}", idx);
            
            for site_manager in &server_instance.site_managers {
                let active_names = site_manager.get_active_middleware_names();
                info!(target: "module_load_manager", 
                      "Site '{}' has {} active middleware: {:?}", 
                      site_manager.site_name(), active_names.len(), active_names);
                
                for name in active_names {
                    middleware_names.insert(name);
                }
            }
        }
        
        let mut result: Vec<String> = middleware_names.into_iter().collect();
        result.sort(); // Sort for consistency
        result
    }
    
    /// Extract unique middleware names from server instance (backward compatibility)
    fn extract_middleware_names(server_instance: &ServerInstance) -> Vec<String> {
        Self::extract_middleware_names_from_instances(&[server_instance.clone()])
    }
    
    /// Load module by name and add to global storage
    fn load_module_by_name_and_add_to_global(&mut self, name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let module_path = self.get_module_path(name)?;
        
        info!(target: "module_load_manager", 
              "Loading middleware module '{}' from: {}", name, module_path.display());
        
        // Safety: Dynamic library loading is unsafe by nature
        let library = unsafe { Library::new(&module_path)? };
        
        let loaded_module = LoadedModule {
            name: name.to_string(),
            library: Arc::new(library),
            path: module_path.clone(),
        };
        
        // Add to global storage
        with_global_storage(|global_storage| {
            // Create a new LoadedModule for global storage (need to reload library)
            if let Ok(global_library) = unsafe { Library::new(&module_path) } {
                let global_module = LoadedModule {
                    name: name.to_string(),
                    library: Arc::new(global_library),
                    path: module_path.clone(),
                };
                global_storage.add_module(global_module);
                info!(target: "module_load_manager", "Added module '{}' to global storage", name);
            } else {
                warn!(target: "module_load_manager", "Failed to create global library instance for '{}'", name);
            }
        }).unwrap_or_else(|e| {
            error!(target: "module_load_manager", "Failed to add module to global storage: {}", e);
        });
        
        // Keep local reference
        self.loaded_modules.push(loaded_module);
        
        info!(target: "module_load_manager", 
              "Successfully loaded and stored middleware module: {}", name);
        
        Ok(())
    }
    
    /// Get module file path based on middleware name and platform
    fn get_module_path(&self, name: &str) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
        let extension = if cfg!(target_os = "windows") {
            "dll"
        } else if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        
        let module_file = format!("{}.{}", name, extension);
        let module_path = self.modules_dir.join(module_file);
        
        if !module_path.exists() {
            return Err(format!("Module file not found: {}", module_path.display()).into());
        }
        
        Ok(module_path)
    }
    
    /// Get reference to loaded modules
    pub fn loaded_modules(&self) -> &[LoadedModule] {
        &self.loaded_modules
    }
    
    /// Get loaded module by name
    pub fn get_module(&self, name: &str) -> Option<&LoadedModule> {
        self.loaded_modules.iter().find(|m| m.name == name)
    }
    
    /// Get middleware instance by name from global storage
    pub fn get_middleware_instance(&self, name: &str) -> Option<MiddlewareModuleInstance> {
        if let Ok(guard) = super::middleware_global_module_storage::with_global_storage(|storage| {
            storage.get_middleware_instance(name)
        }) {
            guard
        } else {
            error!(target: "module_load_manager", "Failed to access global storage for middleware '{}'", name);
            None
        }
    }
    
    /// Check if module is loaded (either locally or in global storage)
    pub fn is_module_loaded(&self, name: &str) -> bool {
        // Check local storage first
        if self.loaded_modules.iter().any(|m| m.name == name) {
            return true;
        }
        
        // Check global storage
        if let Ok(guard) = super::middleware_global_module_storage::with_global_storage(|storage| {
            storage.has_module(name)
        }) {
            guard
        } else {
            false
        }
    }
    
    /// Get count of loaded modules
    pub fn module_count(&self) -> usize {
        self.loaded_modules.len()
    }
    
    /// Get all loaded module names
    pub fn module_names(&self) -> Vec<String> {
        self.loaded_modules.iter().map(|m| m.name.clone()).collect()
    }
    
    /// Create manager with custom modules directory
    pub fn with_modules_dir<P: AsRef<Path>>(modules_dir: P) -> Self {
        Self {
            loaded_modules: Vec::new(),
            modules_dir: modules_dir.as_ref().to_path_buf(),
        }
    }
    
    /// Load modules from server instance with custom directory
    pub fn load_from_server_instance<P: AsRef<Path>>(
        server_instance: &ServerInstance, 
        modules_dir: P
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut manager = Self::with_modules_dir(modules_dir);
        
        let middleware_names = Self::extract_middleware_names(server_instance);
        
        for name in middleware_names {
            if let Err(e) = manager.load_module_by_name_and_add_to_global(&name) {
                error!(target: "module_load_manager", 
                       "Failed to load module '{}': {}", name, e);
            }
        }
        
        Ok(manager)
    }
    
    /// Reload a specific module
    pub fn reload_module(&mut self, name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Remove existing module if it exists
        self.loaded_modules.retain(|m| m.name != name);
        
        // Load it again and add to global storage
        self.load_module_by_name_and_add_to_global(name)
    }
    
    /// Unload a specific module
    pub fn unload_module(&mut self, name: &str) -> bool {
        let initial_len = self.loaded_modules.len();
        self.loaded_modules.retain(|m| m.name != name);
        let removed = self.loaded_modules.len() < initial_len;
        
        if removed {
            info!(target: "module_load_manager", "Unloaded module: {}", name);
        } else {
            warn!(target: "module_load_manager", "Module '{}' was not loaded", name);
        }
        
        removed
    }
}
