use crate::config::strategy::MiddlewareConfig;
use crate::core::HttpWardContext;
use rama::Context;
use rama::http::{Request, Body};
use serde::de::DeserializeOwned;
use std::error::Error;

/// Convenient trait for getting middleware configuration from context
pub trait MiddlewareConfigExt {
    /// Get middleware configuration for current route by name
    fn get_middleware_config(&self, middleware_name: &str) -> Result<Option<MiddlewareConfig>, Box<dyn Error + Send + Sync>>;
    
    /// Get and parse middleware configuration into specific type
    fn get_middleware_config_typed<T>(&self, middleware_name: &str) -> Result<Option<T>, Box<dyn Error + Send + Sync>>
    where
        T: DeserializeOwned;
    
    /// Get middleware configuration by name and path
    fn get_middleware_config_for_path(&self, path: &str, middleware_name: &str) -> Result<Option<MiddlewareConfig>, Box<dyn Error + Send + Sync>>;
    
    /// Get and parse middleware configuration for specific path
    fn get_middleware_config_for_path_typed<T>(&self, path: &str, middleware_name: &str) -> Result<Option<T>, Box<dyn Error + Send + Sync>>
    where
        T: DeserializeOwned;
}

impl MiddlewareConfigExt for Context<()> {
    fn get_middleware_config(&self, middleware_name: &str) -> Result<Option<MiddlewareConfig>, Box<dyn Error + Send + Sync>> {
        let httpward_ctx = self.get::<HttpWardContext>()
            .ok_or("HttpWardContext not found")?;
        
        // Get path from request or use current route
        if let Some((path, _)) = self.get::<(String, ())>() {
            // If path is saved in context, use it
            if let Some(site) = &httpward_ctx.current_site {
                site.get_active_strategy_config_by_route(&path, middleware_name)
                    .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
            } else {
                Err("No current site".into())
            }
        } else {
            // Otherwise try to get from current route
            Err("No path information available in context".into())
        }
    }
    
    fn get_middleware_config_typed<T>(&self, middleware_name: &str) -> Result<Option<T>, Box<dyn Error + Send + Sync>>
    where
        T: DeserializeOwned,
    {
        if let Some(config) = self.get_middleware_config(middleware_name)? {
            let typed_config: T = config.config_into()?;
            Ok(Some(typed_config))
        } else {
            Ok(None)
        }
    }
    
    fn get_middleware_config_for_path(&self, path: &str, middleware_name: &str) -> Result<Option<MiddlewareConfig>, Box<dyn Error + Send + Sync>> {
        let httpward_ctx = self.get::<HttpWardContext>()
            .ok_or("HttpWardContext not found")?;
        
        if let Some(site) = &httpward_ctx.current_site {
            site.get_active_strategy_config_by_route(path, middleware_name)
                .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
        } else {
            Err("No current site".into())
        }
    }
    
    fn get_middleware_config_for_path_typed<T>(&self, path: &str, middleware_name: &str) -> Result<Option<T>, Box<dyn Error + Send + Sync>>
    where
        T: DeserializeOwned,
    {
        if let Some(config) = self.get_middleware_config_for_path(path, middleware_name)? {
            let typed_config: T = config.config_into()?;
            Ok(Some(typed_config))
        } else {
            Ok(None)
        }
    }
}

/// Universal function to get middleware configuration from context
/// Usage: get_config_from_ctx::<MyConfig>(ctx, req, "my_middleware_name")
pub fn get_config_from_ctx<T>(
    ctx: &Context<()>,
    req: &Request<Body>,
    middleware_name: &str,
) -> Option<T>
where
    T: DeserializeOwned,
{
    let path = req.uri().path();
    ctx.get_middleware_config_for_path_typed::<T>(path, middleware_name).unwrap_or_else(|e| {
        None
    })
}

/// Universal function to get middleware configuration from context with module name
/// Usage: get_config_from_ctx_for_module::<MyConfig>(ctx, req, "my_module_name")
pub fn get_config_from_ctx_for_module<T>(
    ctx: &Context<()>,
    req: &Request<Body>,
    module_name: &str,
) -> Option<T>
where
    T: DeserializeOwned,
{
    get_config_from_ctx::<T>(ctx, req, module_name)
}

/// Convenient function for getting middleware configuration from context
pub fn get_middleware_config_from_ctx(
    ctx: &Context<()>,
    middleware_name: &str,
) -> Result<Option<MiddlewareConfig>, Box<dyn Error + Send + Sync>> {
    ctx.get_middleware_config(middleware_name)
}

/// Convenient function for getting typed configuration
pub fn get_middleware_config_typed_from_ctx<T>(
    ctx: &Context<()>,
    middleware_name: &str,
) -> Result<Option<T>, Box<dyn Error + Send + Sync>>
where
    T: DeserializeOwned,
{
    ctx.get_middleware_config_typed(middleware_name)
}
