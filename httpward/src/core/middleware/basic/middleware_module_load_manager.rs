use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use libloading::Library;
use httpward_core::core::server_models::ServerInstance;
use httpward_core::httpward_middleware::middleware_trait::HttpWardMiddleware;
use tracing::{info, error, warn};
use super::middleware_module_instance::MiddlewareModuleInstance;
use super::middleware_global_module_storage::{initialize_global_storage, with_global_storage, ModuleRecord};

const DEFAULT_MODULES_DIR: &str = "modules";

/// Manager for dynamic loading of middleware modules based on strategy configuration
/// Works only with GlobalModuleStorage, doesn't store any local data
#[derive(Debug)]
pub struct MiddlewareModuleLoadManager {
    /// Modules directory path
    modules_dir: PathBuf,
}

impl MiddlewareModuleLoadManager {
    /// Create new module load manager from multiple server instances with dynamic loading (default)
    /// This is the recommended method for most use cases
    pub fn from_server_instances(server_instances: &[ServerInstance]) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::from_server_instances_with_dir(server_instances, Path::new(DEFAULT_MODULES_DIR))
    }
    
    /// Create new module load manager from multiple server instances with static modules
    /// Use this when you want to load built-in modules instead of dynamic ones
    pub fn from_server_instances_statically(
        server_instances: &[ServerInstance],
        static_modules: Vec<(&str, Arc<dyn HttpWardMiddleware + Send + Sync>)>
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::from_server_instances_with_dir_statically(server_instances, Path::new("modules"), static_modules)
    }
    
    /// Create new module load manager from multiple server instances with custom modules directory (dynamic)
    pub fn from_server_instances_with_dir<P: AsRef<Path>>(
        server_instances: &[ServerInstance], 
        modules_dir: P
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let modules_dir = modules_dir.as_ref().to_path_buf();
        
        // Initialize global storage if not already done
        initialize_global_storage()?;
        
        let mut manager = Self {
            modules_dir,
        };
        
        // Load modules dynamically by default
        manager.load_dynamically(server_instances)?;
        
        Ok(manager)
    }
    
    /// Create new module load manager from multiple server instances with custom modules directory (static)
    pub fn from_server_instances_with_dir_statically<P: AsRef<Path>>(
        _server_instances: &[ServerInstance], 
        modules_dir: P,
        static_modules: Vec<(&str, Arc<dyn HttpWardMiddleware + Send + Sync>)>
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let modules_dir = modules_dir.as_ref().to_path_buf();
        
        // Initialize global storage if not already done
        initialize_global_storage()?;
        
        let mut manager = Self {
            modules_dir,
        };
        
        // Load static modules from provided list
        manager.load_static_modules(static_modules)?;
        
        Ok(manager)
    }
    
    /// Create new module load manager from single server instance (for backward compatibility)
    pub fn from_server_instance(server_instance: &ServerInstance) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::from_server_instances(std::slice::from_ref(server_instance))
    }
    
    /// Create new module load manager from single server instance with custom modules directory
    pub fn from_server_instance_with_dir<P: AsRef<Path>>(
        server_instance: &ServerInstance,
        modules_dir: P
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::from_server_instances_with_dir(std::slice::from_ref(server_instance), modules_dir)
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
                      site_manager.site_domains(), active_names.len(), active_names);

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
        Self::extract_middleware_names_from_instances(std::slice::from_ref(server_instance))
    }

    /// Ensure global storage is available before any operation.
    fn ensure_storage() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        initialize_global_storage()
    }

    /// Check if a module already exists in global storage.
    fn is_module_registered(name: &str) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        with_global_storage(|storage| storage.has_module(name))
    }

    /// Store module record in global storage.
    fn store_module_record(module_record: ModuleRecord) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let module_name = module_record.name.clone();
        with_global_storage(|global_storage| {
            global_storage.add_module(module_record);
            info!(target: "module_load_manager", "Added module '{}' to global storage", module_name);
        }).map_err(|e| {
            error!(target: "module_load_manager", "Failed to add module to global storage: {}", e);
            e
        })
    }

    /// Build a dynamic module record by loading a shared library module.
    fn load_dynamic_record(&self, name: &str) -> Result<ModuleRecord, Box<dyn std::error::Error + Send + Sync>> {
        let module_path = self.get_module_path(name)?;

        info!(target: "module_load_manager",
              "Loading middleware module '{}' from: {}", name, module_path.display());

        let lib = Arc::new(unsafe { Library::new(&module_path)? });
        let instance = unsafe { MiddlewareModuleInstance::create_from_arc(lib.clone()) }?;
        let boxed_middleware = instance.into_boxed_middleware();

        Ok(ModuleRecord::dynamic(
            name.to_string(),
            lib,
            boxed_middleware,
            module_path,
        ))
    }

    /// Load module by name and add to global storage
    fn load_module_by_name_and_add_to_global(&mut self, name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Self::ensure_storage()?;

        if Self::is_module_registered(name)? {
            info!(target: "module_load_manager", "Module '{}' already loaded in global storage", name);
            return Ok(());
        }

        let module_record = self.load_dynamic_record(name)?;
        Self::store_module_record(module_record)?;

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
    
    /// Check if module is loaded in global storage
    pub fn is_module_loaded(&self, name: &str) -> bool {
        // Check global storage only
        if let Ok(guard) = super::middleware_global_module_storage::with_global_storage(|storage| {
            storage.has_module(name)
        }) {
            guard
        } else {
            false
        }
    }
    
    /// Get the current modules directory path
    pub fn modules_dir(&self) -> &PathBuf {
        &self.modules_dir
    }
    
    /// Set a new modules directory path
    pub fn set_modules_dir<P: AsRef<Path>>(&mut self, modules_dir: P) {
        let old_dir = self.modules_dir.clone();
        self.modules_dir = modules_dir.as_ref().to_path_buf();
        info!(target: "module_load_manager", 
              "Changed modules directory from '{}' to '{}'", 
              old_dir.display(), self.modules_dir.display());
    }
    
    /// Create manager with custom modules directory
    pub fn with_modules_dir<P: AsRef<Path>>(modules_dir: P) -> Self {
        Self {
            modules_dir: modules_dir.as_ref().to_path_buf(),
        }
    }
    
    /// Load modules from server instance with custom directory
    pub fn load_from_server_instance<P: AsRef<Path>>(
        server_instance: &ServerInstance, 
        modules_dir: P
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut manager = Self::with_modules_dir(modules_dir);
        Self::ensure_storage()?;

        let middleware_names = Self::extract_middleware_names(server_instance);
        
        for name in middleware_names {
            if let Err(e) = manager.load_module_by_name_and_add_to_global(&name) {
                error!(target: "module_load_manager", 
                       "Failed to load module '{}': {}", name, e);
            }
        }
        
        Ok(manager)
    }
    
    /// Get middleware instance by name from global storage
    pub fn get_middleware_instance(&self, name: &str) -> Option<Arc<dyn HttpWardMiddleware + Send + Sync>> {
        use super::middleware_global_module_storage::get_middleware_instance;
        get_middleware_instance(name)
    }
    
    /// Get count of loaded modules from global storage
    pub fn module_count(&self) -> usize {
        if let Ok(count) = super::middleware_global_module_storage::with_global_storage(|storage| {
            storage.module_count()
        }) {
            count
        } else {
            0
        }
    }
    
    /// Get all loaded module names from global storage
    pub fn module_names(&self) -> Vec<String> {
        if let Ok(names) = super::middleware_global_module_storage::with_global_storage(|storage| {
            storage.module_names()
        }) {
            names
        } else {
            Vec::new()
        }
    }
    
    /// Reload a specific module in global storage
    pub fn reload_module(&mut self, name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Note: GlobalModuleStorage doesn't support removal, so we just load it again
        // This will replace the existing module if it exists
        info!(target: "module_load_manager", "Reloading module '{}' in global storage", name);
        self.load_module_by_name_and_add_to_global(name)
    }
    
    /// Unload a specific module from global storage
    pub fn unload_module(&mut self, name: &str) -> bool {
        let result = with_global_storage(|global_storage| global_storage.remove_module(name));
        match result {
            Ok(Some(_)) => {
                info!(target: "module_load_manager", "Module '{}' removed from global storage", name);
                true
            }
            Ok(None) => {
                warn!(target: "module_load_manager", "Module '{}' not found in global storage", name);
                false
            }
            Err(e) => {
                error!(target: "module_load_manager", "Failed to remove module '{}': {}", name, e);
                false
            }
        }
    }
    
    /// Load modules dynamically from DLL files based on server instances
    /// This is the default behavior that loads modules from the modules directory
    pub fn load_dynamically(&mut self, server_instances: &[ServerInstance]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(target: "module_load_manager", "Loading modules dynamically from DLL files");
        
        // Initialize global storage if not already done
        initialize_global_storage()?;
        
        // Extract unique middleware names from all server instances
        let middleware_names = Self::extract_middleware_names_from_instances(server_instances);
        
        info!(target: "module_load_manager", "Found {} unique middleware names from {} server instances: {:?}", 
              middleware_names.len(), server_instances.len(), middleware_names);
        
        // Load modules for each middleware name and add to global storage
        for name in &middleware_names {
            if let Err(e) = self.load_module_by_name_and_add_to_global(name) {
                error!(target: "module_load_manager",
                       "Failed to load module '{}': {}", name, e);
                return Err(e);
            }
        }
        
        info!(target: "module_load_manager", 
              "Successfully loaded {} middleware modules dynamically", middleware_names.len());
        
        Ok(())
    }
    
    /// Load static modules from provided middleware instances
    /// Takes a list of (name, middleware) tuples
    pub fn load_static_modules(&mut self, static_modules: Vec<(&str, Arc<dyn HttpWardMiddleware + Send + Sync>)>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let module_count = static_modules.len();
        info!(target: "module_load_manager", "Loading {} static modules", module_count);
        
        // Initialize global storage if not already done
        Self::ensure_storage()?;

        for (module_name, middleware) in static_modules {
            info!(target: "module_load_manager", "Loading static module: {}", module_name);

            let module_record = ModuleRecord::static_module(module_name.to_string(), middleware);
            Self::store_module_record(module_record)?;
        }
        
        info!(target: "module_load_manager", 
              "Successfully loaded {} static middleware modules", module_count);
        
        Ok(())
    }
}
