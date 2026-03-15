use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use httpward_core::httpward_middleware::{
    HttpWardMiddlewarePipe
};
use rama::{
    http::{Body, Request, Response},
    layer::Layer,
    service::Service,
    Context,
};
use std::fmt::Debug;
use httpward_core::httpward_middleware::pipe::HttpWardMiddlewarePipeBuilder;
use std::fs;

/// Re-export the plugin loader
use super::module_loader::LoadedPlugin;

/// Layer that dynamically loads and applies HttpWard middleware modules
/// This layer integrates the abstract middleware pipe with Rama's layer system
#[derive(Debug)]
pub struct DynamicModuleLoaderLayer {
    middleware_pipe: HttpWardMiddlewarePipe,
    // Keep track of loaded plugin paths for introspection or future use
    loaded_plugin_paths: Vec<PathBuf>,
}

impl DynamicModuleLoaderLayer {
    pub fn new() -> Self {
        let pipe = HttpWardMiddlewarePipeBuilder::new().build();
        let mut loader = Self {
            middleware_pipe: pipe,
            loaded_plugin_paths: Vec::new(),
        };

        // Load plugins from ./plugins directory
        let plugins_dir = Path::new("target/debug/plugins");
        tracing::info!(target: "dynamic_module_loader", "Attempting to load plugins from directory: {}", plugins_dir.display());
        if plugins_dir.exists() {
            tracing::info!(target: "dynamic_module_loader", "Plugins directory exists, loading plugins...");
            match loader.load_plugins_from_dir(plugins_dir) {
                Ok(loaded_paths) => {
                    tracing::info!(target: "dynamic_module_loader", "Successfully loaded {} plugins: {:?}", loaded_paths.len(), loaded_paths);
                }
                Err(e) => {
                    tracing::error!(target: "dynamic_module_loader", "Failed to load plugins from directory {}: {}", plugins_dir.display(), e);
                }
            }
        } else {
            tracing::warn!(target: "dynamic_module_loader", "Plugins directory {} does not exist", plugins_dir.display());
        }

        tracing::info!(target: "dynamic_module_loader", "DynamicModuleLoaderLayer initialized with {} middleware layers", loader.middleware_count());
        loader
    }

    /// Create a new loader with custom middleware pipe
    pub fn with_pipe(pipe: HttpWardMiddlewarePipe) -> Self {
        Self {
            middleware_pipe: pipe,
            loaded_plugin_paths: Vec::new(),
        }
    }

    /// Dynamically add any layer type to the pipe (convenience generic).
    pub fn add_layer<T>(mut self, layer: T) -> Self
    where
        T: httpward_core::httpward_middleware::HttpWardMiddleware + 'static,
    {
        self.middleware_pipe = self.middleware_pipe.add_layer(layer);
        self
    }

    /// Add a pre-boxed middleware (Arc<dyn HttpWardMiddleware>) to the pipe at runtime.
    /// This uses the `add_boxed_layer` method added to pipe.
    pub fn add_boxed_layer(&mut self, boxed: Arc<dyn httpward_core::httpward_middleware::HttpWardMiddleware + Send + Sync>) {
        // Convert Arc to the internal BoxedMiddleware type (which is Arc<dyn ...> already)
        self.middleware_pipe = self.middleware_pipe.add_boxed_layer(boxed);
    }

    /// Load a single plugin by filesystem path and register its middleware into the pipe.
    /// Returns the plugin path on success (for bookkeeping).
    pub fn load_plugin_from_path(&mut self, path: &Path) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
        tracing::info!(target: "dynamic_module_loader", "Loading plugin from path: {}", path.display());
        // Safety: Plugin loading via libloading is unsafe. We do it in an isolated helper.
        unsafe {
            let loaded = LoadedPlugin::load(path)?;
            let boxed = loaded.into_boxed_middleware();
            // Register into pipe
            self.middleware_pipe = self.middleware_pipe.add_boxed_layer(boxed);
            let canonical = path.to_path_buf();
            self.loaded_plugin_paths.push(canonical.clone());
            tracing::info!(target: "dynamic_module_loader", "Successfully loaded plugin: {}", path.display());
            Ok(canonical)
        }
    }

    /// Load all plugins from a directory (files with .so / .dylib / .dll suffixes).
    /// Returns list of loaded paths.
    pub fn load_plugins_from_dir(&mut self, dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error + Send + Sync>> {
        let mut loaded = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let ext_ok = if cfg!(target_os = "windows") {
                    path.extension().map(|e| e == "dll").unwrap_or(false)
                } else if cfg!(target_os = "macos") {
                    path.extension().map(|e| e == "dylib").unwrap_or(false)
                } else {
                    path.extension().map(|e| e == "so").unwrap_or(false)
                };
                if ext_ok {
                    match self.load_plugin_from_path(&path) {
                        Ok(p) => loaded.push(p),
                        Err(e) => {
                            tracing::error!(target: "dynamic_module_loader", path = %path.display(), error = %e, "failed to load plugin");
                        }
                    }
                }
            }
        }
        Ok(loaded)
    }

    /// Get the number of middleware layers
    pub fn middleware_count(&self) -> usize {
        self.middleware_pipe.len()
    }

    /// Check if any middleware is configured
    pub fn has_middleware(&self) -> bool {
        !self.middleware_pipe.is_empty()
    }

    /// Get a reference to the middleware pipe
    pub fn pipe(&self) -> &HttpWardMiddlewarePipe {
        &self.middleware_pipe
    }

    /// Get a specific layer by name
    pub fn get_layer_by_name(&self, name: &str) -> Option<std::sync::Arc<dyn httpward_core::httpward_middleware::HttpWardMiddleware>> {
        self.middleware_pipe.get_layer_by_name(name).cloned()
    }

    /// Get loaded plugin paths for introspection
    pub fn loaded_plugins(&self) -> &[PathBuf] {
        &self.loaded_plugin_paths
    }
}

impl Default for DynamicModuleLoaderLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for DynamicModuleLoaderLayer
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Send + Sync + 'static,
{
    type Service = DynamicModuleLoaderService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DynamicModuleLoaderService::new(inner, &self.middleware_pipe)
    }
}

/// Service that applies dynamic middleware modules to requests and responses
#[derive(Debug)]
pub struct DynamicModuleLoaderService<S> {
    inner: S,
    middleware_pipe: HttpWardMiddlewarePipe,
}

impl<S> DynamicModuleLoaderService<S>
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Send + Sync + 'static,
{
    pub fn new(inner: S, middleware_pipe: &HttpWardMiddlewarePipe) -> Self {
        Self {
            inner,
            middleware_pipe: middleware_pipe.clone(), // Use the passed pipe instead of empty one
        }
    }

    /// Get the number of middleware layers
    pub fn middleware_count(&self) -> usize {
        self.middleware_pipe.len()
    }

    /// Get a reference to the middleware pipe
    pub fn pipe(&self) -> &HttpWardMiddlewarePipe {
        &self.middleware_pipe
    }

    /// Get a specific layer by name
    pub fn get_layer_by_name(&self, name: &str) -> Option<std::sync::Arc<dyn httpward_core::httpward_middleware::HttpWardMiddleware>> {
        self.middleware_pipe.get_layer_by_name(name).cloned()
    }
}

impl<S> Service<(), Request<Body>> for DynamicModuleLoaderService<S>
where
    S: Service<(), Request<Body>, Response = Response<Body>> + Clone + Send + Sync + 'static,
    S::Error: Debug + Send + Sync + std::error::Error + 'static,
    S::Response: Debug + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn serve(
        &self,
        ctx: Context<()>,
        request: Request<Body>,
    ) -> Result<Self::Response, Self::Error> {
        tracing::debug!(target: "dynamic_module_loader", "DynamicModuleLoaderService.serve called");

        // Execute all middleware in the pipe automatically
        // The pipe handles all the nested middleware logic
        self.middleware_pipe.execute_middleware(self.inner.clone(), ctx, request).await
    }
}
