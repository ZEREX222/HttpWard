use serde::{Deserialize, Serialize, Deserializer};
use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use schemars::JsonSchema;
use serde_yaml::Value;
use serde_json;

/// Universal wrapper type for working with YAML and JSON values
#[derive(Debug, Clone)]
pub enum UniversalValue {
    Json(serde_json::Value),
    Yaml(serde_yaml::Value),
}

impl UniversalValue {
    /// Convert to JSON Value
    pub fn as_json(&self) -> Result<serde_json::Value> {
        match self {
            UniversalValue::Json(v) => Ok(v.clone()),
            UniversalValue::Yaml(v) => {
                // Use direct conversion method via serde_json::to_value
                serde_json::to_value(v).map_err(|e| {
                    anyhow::anyhow!("Failed to convert YAML to JSON: {}", e)
                })
            }
        }
    }
    
    /// Convert to YAML Value
    pub fn as_yaml(&self) -> Result<serde_yaml::Value> {
        match self {
            UniversalValue::Yaml(v) => Ok(v.clone()),
            UniversalValue::Json(v) => {
                // Use direct conversion method via serde_yaml::to_value
                serde_yaml::to_value(v).map_err(|e| {
                    anyhow::anyhow!("Failed to convert JSON to YAML: {}", e)
                })
            }
        }
    }
    
    /// Create from JSON Value
    pub fn from_json(value: serde_json::Value) -> Self {
        UniversalValue::Json(value)
    }
    
    /// Create from YAML Value
    pub fn from_yaml(value: serde_yaml::Value) -> Self {
        UniversalValue::Yaml(value)
    }
    
    /// Get as JSON string
    pub fn to_json_string(&self) -> Result<String> {
        let json_val = self.as_json()?;
        serde_json::to_string_pretty(&json_val).map_err(Into::into)
    }
    
    /// Get as YAML string
    pub fn to_yaml_string(&self) -> Result<String> {
        let yaml_val = self.as_yaml()?;
        serde_yaml::to_string(&yaml_val).map_err(Into::into)
    }
}

/// Deep merge YAML values - only add missing properties
fn merge_yaml_missing_only(target: &mut Value, source: Value) {
    let is_null = matches!(target, Value::Null);
    
    if let (Value::Mapping(target_map), Value::Mapping(source_map)) = (&mut *target, &source) {
        for (k, v) in source_map {
            if !target_map.contains_key(&k) {
                // Key doesn't exist at all - add it
                target_map.insert(k.clone(), v.clone());
            } else {
                // Key exists - check if we can recursively merge objects
                match (target_map.get_mut(&k), &v) {
                    (Some(Value::Mapping(_)), Value::Mapping(_)) => {
                        // Both are mappings - recursively merge missing fields only
                        let target_value = target_map.get_mut(&k).unwrap();
                        merge_yaml_missing_only(target_value, v.clone());
                    }
                    _ => {
                        // Target is not a mapping or source is not a mapping - don't overwrite
                    }
                }
            }
        }
    } else if is_null {
        // If target is null but source has value, replace target
        *target = source;
    }
    // Don't modify existing non-null values
}

/// Deep merge JSON values - only add missing properties
fn merge_json_missing_only(target: &mut serde_json::Value, source: serde_json::Value) {
    let is_null = matches!(target, serde_json::Value::Null);
    
    if let (serde_json::Value::Object(target_map), serde_json::Value::Object(source_map)) = (&mut *target, &source) {
        for (k, v) in source_map {
            if !target_map.contains_key(k) {
                // Key doesn't exist at all - add it
                target_map.insert(k.clone(), v.clone());
            } else {
                // Key exists - check if we can recursively merge objects
                let target_value = target_map.get_mut(k).unwrap();
                match target_value {
                    serde_json::Value::Object(_) => {
                        // Both are objects - recursively merge missing fields only
                        merge_json_missing_only(target_value, v.clone());
                    }
                    _ => {
                        // Target is not an object - don't overwrite
                    }
                }
            }
        }
    } else if is_null {
        // If target is null but source has value, replace target
        *target = source;
    }
    // Don't modify existing non-null values
}

/// Deep merge UniversalValue instances - only add missing properties
fn merge_universal_missing_only(target: &mut UniversalValue, source: UniversalValue) -> Result<()> {
    match (target, source) {
        (UniversalValue::Yaml(t), UniversalValue::Yaml(s)) => {
            merge_yaml_missing_only(t, s);
        }
        (UniversalValue::Json(t), UniversalValue::Json(s)) => {
            merge_json_missing_only(t, s);
        }
        // Fallback if formats are different
        (t, s) => {
            let t_json = t.as_json()?;
            let s_json = s.as_json()?;
            let mut merged = t_json;
            merge_json_missing_only(&mut merged, s_json);
            *t = UniversalValue::from_json(merged);
        }
    }
    Ok(())
}

/// Supplement middleware configurations - only merge existing middleware configs, don't add new ones
/// Used for strategy inheritance where we only want to merge configs of existing middleware
pub fn supplement_middleware_configs(
    current: &mut Vec<MiddlewareConfig>,
    incoming: &[MiddlewareConfig],
) -> Result<()> {
    // Build index: middleware name -> position
    let mut index = HashMap::new();

    for (i, m) in current.iter().enumerate() {
        let name = m.name();
        index.insert(name.to_string(), i);
    }

    for new_middleware in incoming {
        let name = new_middleware.name();
        
        if let Some(&pos) = index.get(name) {
            // Only merge existing middleware configs - don't add new ones
            match (&mut current[pos], new_middleware) {
                (MiddlewareConfig::Named { config: existing_config, .. }, MiddlewareConfig::Named { config, .. }) => {
                    merge_universal_missing_only(existing_config, config.clone())?;
                }
                (current_on @ MiddlewareConfig::On { .. }, MiddlewareConfig::Named { name, config }) => {
                    *current_on = MiddlewareConfig::Named {
                        name: name.clone(),
                        config: config.clone(),
                    };
                }
                // Handle cases where one or both are Off - don't merge configs for Off middleware
                _ => {
                    // Do nothing - either current or incoming is Off, so no config merging needed
                }
            }
        }
        // If middleware doesn't exist in current, don't add it (this function only supplements existing)
    }

    Ok(())
}

/// Filter out disabled middleware, considering inheritance rules
/// If middleware is disabled in current but enabled in parent, it stays disabled
/// If middleware is disabled in parent but enabled in current, it becomes enabled
pub fn filter_disabled_middleware(
    current: &mut Vec<MiddlewareConfig>,
    parent: &[MiddlewareConfig],
) -> Result<()> {
    // Build index of parent middleware: name -> (is_disabled, config)
    let mut parent_index = HashMap::new();
    for middleware in parent {
        let name = middleware.name();
        parent_index.insert(name.to_string(), middleware.is_off());
    }

    // Process current middleware
    let mut result = Vec::new();
    
    for middleware in current.iter() {
        let name = middleware.name();
        
        match middleware {
            MiddlewareConfig::Off { .. } => {
                // Current middleware is disabled - check parent
                if let Some(parent_disabled) = parent_index.get(name) {
                    if !parent_disabled {
                        // Parent has it enabled, so keep it disabled (current takes precedence)
                        result.push(middleware.clone());
                    }
                    // If parent also has it disabled, don't add anything (remove it)
                } else {
                    // Parent doesn't have this middleware, keep it disabled
                    result.push(middleware.clone());
                }
            }
            MiddlewareConfig::Named { .. } | MiddlewareConfig::On { .. } => {
                // Current middleware is enabled - always keep it
                result.push(middleware.clone());
            }
        }
    }

    // Add parent middleware that doesn't exist in current (if not disabled)
    for middleware in parent {
        let name = middleware.name();
        
        // Check if this middleware exists in current
        let exists_in_current = current.iter().any(|m| m.name() == name);
        
        if !exists_in_current {
            match middleware {
                MiddlewareConfig::Named { .. } | MiddlewareConfig::On { .. } => {
                    // Parent has it enabled and current doesn't have it - add it
                    result.push(middleware.clone());
                }
                MiddlewareConfig::Off { .. } => {
                    // Parent has it disabled and current doesn't have it - don't add
                }
            }
        }
    }

    *current = result;
    Ok(())
}

/// Supplement middleware configurations - add missing middleware and missing properties
pub fn supplement_middleware(
    current: &mut Vec<MiddlewareConfig>,
    incoming: &[MiddlewareConfig],
) -> Result<()> {
    // Build index: middleware name -> position
    let mut index = HashMap::new();

    for (i, m) in current.iter().enumerate() {
        let name = m.name();
        index.insert(name.to_string(), i);
    }

    // Collect inherited middleware missing in `current` and prepend once at the end.
    // This keeps inheritance order as parent -> child without O(n^2) front inserts.
    let mut inherited_prefix: Vec<MiddlewareConfig> = Vec::new();

    for new_middleware in incoming {
        let name = new_middleware.name();
        
        if let Some(&pos) = index.get(name) {
            // Middleware exists - check if we should merge or handle disabled state
            match (&mut current[pos], new_middleware) {
                (MiddlewareConfig::Named { config: existing_config, .. }, MiddlewareConfig::Named { config, .. }) => {
                    // Both are enabled - merge configurations
                    merge_universal_missing_only(existing_config, config.clone())?;
                }
                (current_on @ MiddlewareConfig::On { .. }, MiddlewareConfig::Named { name, config }) => {
                    // Explicitly enabled without config - inherit missing config from parent.
                    *current_on = MiddlewareConfig::Named {
                        name: name.clone(),
                        config: config.clone(),
                    };
                }
                (MiddlewareConfig::Off { .. }, MiddlewareConfig::Named { .. } | MiddlewareConfig::On { .. }) => {
                    // Current is disabled but incoming is enabled - DON'T enable it
                    // Current (inline/off) takes precedence over incoming (site/global)
                    // Do nothing - keep it disabled
                }
                (MiddlewareConfig::Named { .. } | MiddlewareConfig::On { .. }, MiddlewareConfig::Off { .. }) => {
                    // Current is enabled but incoming is disabled - keep current enabled (takes precedence)
                    // Do nothing
                }
                (MiddlewareConfig::Off { .. }, MiddlewareConfig::Off { .. }) => {
                    // Both are disabled - keep disabled
                    // Do nothing
                }
                (MiddlewareConfig::Named { .. }, MiddlewareConfig::On { .. }) => {
                    // Current already has explicit config - keep it as-is.
                }
                (MiddlewareConfig::On { .. }, MiddlewareConfig::On { .. }) => {
                    // Already explicitly enabled without config.
                }
            }
        } else {
            // Middleware doesn't exist in current - inherit if not disabled.
            if !new_middleware.is_off() {
                inherited_prefix.push(new_middleware.clone());
            }
        }
    }

    if !inherited_prefix.is_empty() {
        let mut merged = Vec::with_capacity(inherited_prefix.len() + current.len());
        merged.extend(inherited_prefix);
        merged.append(current);
        *current = merged;
    }

    Ok(())
}

// Serialize implementation for UniversalValue
impl Serialize for UniversalValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.as_json() {
            Ok(json_val) => json_val.serialize(serializer),
            Err(_) => serializer.serialize_none(),
        }
    }
}

// Deserialize implementation for UniversalValue
impl<'de> Deserialize<'de> for UniversalValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let yaml_value: serde_yaml::Value = Deserialize::deserialize(deserializer)?;
        Ok(UniversalValue::Yaml(yaml_value))
    }
}

// Legacy type for backward compatibility - use the optimized version in strategy_resolver
pub type LegacyStrategyCollection = HashMap<String, Vec<MiddlewareConfig>>;

#[derive(Debug, Clone, JsonSchema)]
pub struct Strategy {
    pub name: String,

    #[serde(default)]
    pub middleware: Arc<Vec<MiddlewareConfig>>,
}

impl<'de> serde::Deserialize<'de> for Strategy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Helper {
            name: String,
            #[serde(default)]
            middleware: Vec<MiddlewareConfig>,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(Strategy {
            name: helper.name,
            middleware: Arc::new(helper.middleware),
        })
    }
}

impl serde::Serialize for Strategy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Strategy", 2)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("middleware", &*self.middleware)?;
        state.end()
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub enum MiddlewareConfig {
    Named {
        name: String,
        #[schemars(with = "serde_json::Value")]
        config: UniversalValue,
    },
    On {
        name: String,
    },
    Off {
        name: String,
    },
}

impl MiddlewareConfig {
    /// Create new named middleware with JSON configuration
    pub fn new_named_json(name: String, config: serde_json::Value) -> Self {
        MiddlewareConfig::Named {
            name,
            config: UniversalValue::from_json(config),
        }
    }
    
    /// Create new named middleware with YAML configuration
    pub fn new_named_yaml(name: String, config: serde_yaml::Value) -> Self {
        MiddlewareConfig::Named {
            name,
            config: UniversalValue::from_yaml(config),
        }
    }

    /// Create new enabled middleware without explicit configuration
    pub fn new_on(name: String) -> Self {
        MiddlewareConfig::On { name }
    }

    /// Create new disabled middleware
    pub fn new_off(name: String) -> Self {
        MiddlewareConfig::Off { name }
    }
    
    /// Check if middleware is disabled
    pub fn is_off(&self) -> bool {
        matches!(self, MiddlewareConfig::Off { .. })
    }
    
    /// Get middleware name
    pub fn name(&self) -> &str {
        match self {
            MiddlewareConfig::Named { name, .. } => name,
            MiddlewareConfig::On { name } => name,
            MiddlewareConfig::Off { name } => name,
        }
    }
    
    /// Get configuration as JSON Value
    pub fn config_as_json(&self) -> Result<serde_json::Value> {
        match self {
            MiddlewareConfig::Named { config, .. } => config.as_json(),
            MiddlewareConfig::On { .. } => Ok(serde_json::Value::Object(serde_json::Map::new())),
            MiddlewareConfig::Off { .. } => Err(anyhow::anyhow!("Cannot get config from disabled middleware")),
        }
    }
    
    /// Get configuration as YAML Value
    pub fn config_as_yaml(&self) -> Result<serde_yaml::Value> {
        match self {
            MiddlewareConfig::Named { config, .. } => config.as_yaml(),
            MiddlewareConfig::On { .. } => Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new())),
            MiddlewareConfig::Off { .. } => Err(anyhow::anyhow!("Cannot get config from disabled middleware")),
        }
    }
    
    /// Get configuration as JSON string
    pub fn config_to_json_string(&self) -> Result<String> {
        match self {
            MiddlewareConfig::Named { config, .. } => config.to_json_string(),
            MiddlewareConfig::On { .. } => serde_json::to_string_pretty(&serde_json::Value::Object(serde_json::Map::new())).map_err(Into::into),
            MiddlewareConfig::Off { .. } => Err(anyhow::anyhow!("Cannot get config from disabled middleware")),
        }
    }
    
    /// Get configuration as YAML string
    pub fn config_to_yaml_string(&self) -> Result<String> {
        match self {
            MiddlewareConfig::Named { config, .. } => config.to_yaml_string(),
            MiddlewareConfig::On { .. } => serde_yaml::to_string(&serde_yaml::Value::Mapping(serde_yaml::Mapping::new())).map_err(Into::into),
            MiddlewareConfig::Off { .. } => Err(anyhow::anyhow!("Cannot get config from disabled middleware")),
        }
    }
    
    /// Convert configuration to specific type using serde
    pub fn config_into<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        let json_val = self.config_as_json()?;
        serde_json::from_value(json_val).map_err(Into::into)
    }

    /// Create middleware config from any serializable struct (super convenient!)
    /// 
    /// Usage:
    /// ```rust
    /// # use httpward_core::config::MiddlewareConfig;
    /// # use serde::Serialize;
    /// # #[derive(Serialize)]
    /// # struct MyConfig { level: String }
    /// let config = MyConfig { level: "warn".to_string() };
    /// let middleware = MiddlewareConfig::from_serializable("httpward_log_module", config).unwrap();
    /// ```
    pub fn from_serializable<T: Serialize>(name: impl Into<String>, config: T) -> Result<Self> {
        let json_val = serde_json::to_value(config)
            .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;
        Ok(MiddlewareConfig::new_named_json(name.into(), json_val))
    }

    /// Create middleware config from YAML string (1-liner friendly)
    /// 
    /// Usage:
    /// ```rust
    /// # use httpward_core::config::MiddlewareConfig;
    /// let middleware = MiddlewareConfig::from_yaml_str("httpward_log_module", "level: warn").unwrap();
    /// ```
    pub fn from_yaml_str(name: impl Into<String>, yaml_str: impl AsRef<str>) -> Result<Self> {
        let yaml_val = serde_yaml::from_str(yaml_str.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to parse YAML: {}", e))?;
        Ok(MiddlewareConfig::new_named_yaml(name.into(), yaml_val))
    }

    /// Create middleware config from JSON string (1-liner friendly)
    /// 
    /// Usage:
    /// ```rust
    /// # use httpward_core::config::MiddlewareConfig;
    /// let middleware = MiddlewareConfig::from_json_str("httpward_log_module", r#"{"level": "warn"}"#).unwrap();
    /// ```
    pub fn from_json_str(name: impl Into<String>, json_str: impl AsRef<str>) -> Result<Self> {
        let json_val = serde_json::from_str(json_str.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to parse JSON: {}", e))?;
        Ok(MiddlewareConfig::new_named_json(name.into(), json_val))
    }

    /// Parse config into specific type with elegant error handling
    /// 
    /// Usage:
    /// ```rust
    /// # use httpward_core::config::MiddlewareConfig;
    /// # use serde::Deserialize;
    /// # #[derive(Deserialize)]
    /// # struct MyConfig { level: String }
    /// let middleware = MiddlewareConfig::from_json_str("httpward_log_module", r#"{"level": "warn"}"#).unwrap();
    /// let config: MyConfig = middleware.parse_config().unwrap();
    /// ```
    pub fn parse_config<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        self.config_into()
    }
}

impl<'de> Deserialize<'de> for MiddlewareConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map: HashMap<String, serde_yaml::Value> =
            HashMap::deserialize(deserializer)?;

        if map.len() != 1 {
            return Err(serde::de::Error::custom(
                "middleware must contain exactly one key",
            ));
        }

        let (name, yaml_value) = map.into_iter().next().unwrap();

        // Check if the value is an explicit on/off toggle.
        match &yaml_value {
            serde_yaml::Value::String(s) if s.eq_ignore_ascii_case("off") => {
                return Ok(MiddlewareConfig::Off { name });
            }
            serde_yaml::Value::Bool(b) if !b => {
                return Ok(MiddlewareConfig::Off { name });
            }
            serde_yaml::Value::String(s) if s.eq_ignore_ascii_case("on") => {
                return Ok(MiddlewareConfig::On { name });
            }
            serde_yaml::Value::Bool(b) if *b => {
                return Ok(MiddlewareConfig::On { name });
            }
            _ => {
                // Normal middleware configuration
                Ok(MiddlewareConfig::Named { 
                    name, 
                    config: UniversalValue::from_yaml(yaml_value)
                })
            }
        }
    }
}

impl Strategy {
    pub fn new(name: String) -> Self {
        Self {
            name,
            middleware: Arc::new(Vec::new()),
        }
    }

    /// Supplement middleware configurations - only add missing middleware and missing properties
    pub fn supplement_with(&mut self, incoming: &[MiddlewareConfig]) -> Result<()> {
        supplement_middleware(Arc::make_mut(&mut self.middleware), incoming)
    }

    pub fn merge_with(&self, other: &Strategy) -> Strategy {
        let mut result = self.clone();

        for middleware in other.middleware.iter() {
            let merged = Arc::make_mut(&mut result.middleware);
            if let Some(pos) = merged.iter().position(|existing| existing.name() == middleware.name()) {
                merged[pos] = middleware.clone();
            } else {
                merged.push(middleware.clone());
            }
        }

        result
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum StrategyRef {
    Named(String),
    InlineMiddleware(Vec<MiddlewareConfig>),
}

impl StrategyRef {
    pub fn resolve(&self, strategies: &LegacyStrategyCollection) -> Option<Strategy> {
        match self {
            StrategyRef::Named(name) => strategies.get(name).map(|middleware| Strategy {
                name: name.clone(),
                middleware: Arc::new(middleware.clone()),
            }),
            StrategyRef::InlineMiddleware(middleware) => Some(Strategy {
                name: "inline".to_string(),
                middleware: Arc::new(middleware.clone()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_universal_value_json_conversion() {
        let json_val = json!({
            "key": "value",
            "number": 42,
            "nested": {
                "array": [1, 2, 3]
            }
        });

        let universal = UniversalValue::from_json(json_val.clone());
        
        // Convert back to JSON
        let converted = universal.as_json().unwrap();
        assert_eq!(json_val, converted);
    }

    #[test]
    fn test_universal_value_yaml_conversion() {
        let yaml_str = r#"key: value
number: 42
nested:
  array: [1, 2, 3]"#;

        let yaml_val: serde_yaml::Value = serde_yaml::from_str(yaml_str).unwrap();
        let universal = UniversalValue::from_yaml(yaml_val);
        
        // Convert to JSON
        let json_val = universal.as_json().unwrap();
        assert_eq!(json_val["key"], "value");
        assert_eq!(json_val["number"], 42);
        assert_eq!(json_val["nested"]["array"][0], 1);
    }

    #[test]
    fn test_universal_value_string_conversion() {
        let json_val = json!({
            "message": "Hello, World!",
            "count": 100
        });

        let universal = UniversalValue::from_json(json_val);
        
        // JSON string
        let json_str = universal.to_json_string().unwrap();
        assert!(json_str.contains("Hello, World!"));
        assert!(json_str.contains("100"));
        
        // YAML string
        let yaml_str = universal.to_yaml_string().unwrap();
        assert!(yaml_str.contains("message: Hello, World!"));
        assert!(yaml_str.contains("count: 100"));
    }

    #[test]
    fn test_middleware_config_with_universal_value() {
        let json_config = json!({
            "requests": 1000,
            "window": "1m"
        });

        let middleware = MiddlewareConfig::new_named_json(
            "rate_limit".to_string(),
            json_config.clone()
        );

        assert_eq!(middleware.name(), "rate_limit");
        
        // Get as JSON
        let retrieved_json = middleware.config_as_json().unwrap();
        assert_eq!(json_config, retrieved_json);
        
        // Convert to specific type
        #[derive(Deserialize)]
        struct RateLimitConfig {
            requests: u32,
            window: String,
        }
        
        let rate_limit: RateLimitConfig = middleware.config_into().unwrap();
        assert_eq!(rate_limit.requests, 1000);
        assert_eq!(rate_limit.window, "1m");
    }

    #[test]
    fn test_middleware_config_yaml_deserialization() {
        let yaml_str = r#"
- rate_limit:
    requests: 500
    window: "30s"
- logging:
    level: info
    format: json
"#;

        let middleware: Vec<MiddlewareConfig> = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(middleware.len(), 2);
        
        // Check first middleware
        let rate_limit = &middleware[0];
        assert_eq!(rate_limit.name(), "rate_limit");
        
        let config_json = rate_limit.config_as_json().unwrap();
        assert_eq!(config_json["requests"], 500);
        assert_eq!(config_json["window"], "30s");
        
        // Check second middleware
        let logging = &middleware[1];
        assert_eq!(logging.name(), "logging");
        
        let config_json = logging.config_as_json().unwrap();
        assert_eq!(config_json["level"], "info");
        assert_eq!(config_json["format"], "json");
    }

    #[test]
    fn test_strategy_with_universal_values() {
        let yaml_strategy = r#"
name: "test_strategy"
middleware:
  - auth:
      type: jwt
      secret: "my-secret"
  - cors:
      origins: ["*"]
      methods: ["GET", "POST"]
"#;

        let strategy: Strategy = serde_yaml::from_str(yaml_strategy).unwrap();
        assert_eq!(strategy.name, "test_strategy");
        assert_eq!(strategy.middleware.len(), 2);
        
        // Check auth middleware
        let auth = &strategy.middleware[0];
        assert_eq!(auth.name(), "auth");
        
        #[derive(Deserialize)]
        struct AuthConfig {
            r#type: String,
            secret: String,
        }
        
        let auth_config: AuthConfig = auth.config_into().unwrap();
        assert_eq!(auth_config.r#type, "jwt");
        assert_eq!(auth_config.secret, "my-secret");
        
        // Check CORS middleware
        let cors = &strategy.middleware[1];
        assert_eq!(cors.name(), "cors");
        
        let cors_json = cors.config_as_json().unwrap();
        assert_eq!(cors_json["origins"][0], "*");
        assert_eq!(cors_json["methods"][0], "GET");
        assert_eq!(cors_json["methods"][1], "POST");
    }

    #[test]
    fn test_strategy_ref_inline_middleware_collection() {
        // Test InlineMiddleware(Vec<MiddlewareConfig>) functionality
        let inline_yaml = r#"
  - rate_limit:
      requests: 100
      window: "1m"
  - logging:
      level: debug
      format: json
"#;

        let strategy_ref: StrategyRef = serde_yaml::from_str(inline_yaml).unwrap();
        let resolved = strategy_ref.resolve(&LegacyStrategyCollection::new());
        
        assert!(resolved.is_some());
        let strategy = resolved.unwrap();
        assert_eq!(strategy.name, "inline");
        assert_eq!(strategy.middleware.len(), 2);
        
        // Check rate_limit middleware
        let rate_limit = &strategy.middleware[0];
        assert_eq!(rate_limit.name(), "rate_limit");
        
        #[derive(Deserialize)]
        struct RateLimitConfig {
            requests: u32,
            window: String,
        }
        
        let rate_config: RateLimitConfig = rate_limit.config_into().unwrap();
        assert_eq!(rate_config.requests, 100);
        assert_eq!(rate_config.window, "1m");
        
        // Check logging middleware
        let logging = &strategy.middleware[1];
        assert_eq!(logging.name(), "logging");
        
        let logging_json = logging.config_as_json().unwrap();
        assert_eq!(logging_json["level"], "debug");
        assert_eq!(logging_json["format"], "json");
    }

    #[test]
    fn test_strategy_ref_named_vs_inline() {
        let mut strategies = LegacyStrategyCollection::new();
        
        // Add a named strategy
        strategies.insert("test".to_string(), vec![
            MiddlewareConfig::new_named_json(
                "auth".to_string(),
                json!({"type": "basic"})
            )
        ]);
        
        // Test Named strategy
        let named_ref = StrategyRef::Named("test".to_string());
        let named_resolved = named_ref.resolve(&strategies).unwrap();
        assert_eq!(named_resolved.name, "test");
        assert_eq!(named_resolved.middleware.len(), 1);
        
        // Test Inline middleware strategy
        let inline_middleware = vec![
            MiddlewareConfig::new_named_json(
                "logging".to_string(),
                json!({"level": "info"})
            )
        ];
        let inline_ref = StrategyRef::InlineMiddleware(inline_middleware);
        let inline_resolved = inline_ref.resolve(&strategies).unwrap();
        assert_eq!(inline_resolved.name, "inline");
        assert_eq!(inline_resolved.middleware.len(), 1);
    }

    #[test]
    fn test_universal_value_roundtrip() {
        let original = json!({
            "string": "test",
            "number": 42,
            "boolean": true,
            "null": null,
            "array": [1, 2, 3],
            "object": {
                "nested": "value"
            }
        });

        // JSON -> UniversalValue -> YAML -> UniversalValue -> JSON
        let universal1 = UniversalValue::from_json(original.clone());
        let yaml_val = universal1.as_yaml().unwrap();
        let universal2 = UniversalValue::from_yaml(yaml_val);
        let final_json = universal2.as_json().unwrap();
        
        // Compare via string since key order may differ
        let original_str = serde_json::to_string(&original).unwrap();
        let final_str = serde_json::to_string(&final_json).unwrap();
        
        // Parse both for value comparison
        let parsed_original: serde_json::Value = serde_json::from_str(&original_str).unwrap();
        let parsed_final: serde_json::Value = serde_json::from_str(&final_str).unwrap();
        
        assert_eq!(parsed_original, parsed_final);
    }

    #[test]
    fn test_supplement_middleware_missing_only() {
        let mut current = vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 1000,
                    "window": "1m",
                    "burst": 100
                })
            ),
            MiddlewareConfig::new_named_json(
                "logging".to_string(),
                json!({
                    "level": "info",
                    "format": "text"
                })
            )
        ];

        let incoming = vec![
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 2000,  // Should NOT be updated (exists)
                    "timeout": "30s"   // Should be added (missing)
                })
            ),
            MiddlewareConfig::new_named_json(
                "cors".to_string(),
                json!({
                    "origins": ["*"],
                    "methods": ["GET", "POST"]
                })
            )
        ];

        supplement_middleware(&mut current, &incoming).unwrap();

        assert_eq!(current.len(), 3); // cors (inherited), rate_limit, logging

        assert_eq!(current[0].name(), "cors");

        // Check rate_limit was supplemented (not merged)
        let rate_limit = current.iter().find(|m| m.name() == "rate_limit").unwrap();
        assert_eq!(rate_limit.name(), "rate_limit");
        let config = rate_limit.config_as_json().unwrap();
        assert_eq!(config["requests"], 1000);  // NOT updated
        assert_eq!(config["window"], "1m");    // Preserved
        assert_eq!(config["burst"], 100);      // Preserved
        assert_eq!(config["timeout"], "30s");  // Added (was missing)

        // Check logging was preserved
        let logging = current.iter().find(|m| m.name() == "logging").unwrap();
        assert_eq!(logging.name(), "logging");
        let config = logging.config_as_json().unwrap();
        assert_eq!(config["level"], "info");
        assert_eq!(config["format"], "text");

        // Check cors was inherited and prepended
        let cors = current.iter().find(|m| m.name() == "cors").unwrap();
        assert_eq!(cors.name(), "cors");
        let config = cors.config_as_json().unwrap();
        assert_eq!(config["origins"][0], "*");
        assert_eq!(config["methods"][0], "GET");
    }

    #[test]
    fn test_strategy_supplement_with_method() {
        let mut strategy = Strategy::new("test".to_string());
        Arc::make_mut(&mut strategy.middleware).push(
            MiddlewareConfig::new_named_json(
                "auth".to_string(),
                json!({
                    "type": "basic",
                    "realm": "protected"
                })
            )
        );

        let incoming = vec![
            MiddlewareConfig::new_named_json(
                "auth".to_string(),
                json!({
                    "timeout": 300,      // Should be added (missing)
                    "max_attempts": 5,    // Should be added (missing)
                    "type": "jwt"         // Should NOT be updated (exists)
                })
            ),
            MiddlewareConfig::new_named_json(
                "rate_limit".to_string(),
                json!({
                    "requests": 100
                })
            )
        ];

        strategy.supplement_with(&incoming).unwrap();

        assert_eq!(strategy.middleware.len(), 2);

        // Check auth was supplemented (not merged)
        let auth = strategy.middleware.iter().find(|m| m.name() == "auth").unwrap();
        assert_eq!(auth.name(), "auth");
        let config = auth.config_as_json().unwrap();
        assert_eq!(config["type"], "basic");      // NOT updated
        assert_eq!(config["realm"], "protected"); // Preserved
        assert_eq!(config["timeout"], 300);       // Added (was missing)
        assert_eq!(config["max_attempts"], 5);     // Added (was missing)

        // Check rate_limit was inherited and prepended
        assert_eq!(strategy.middleware[0].name(), "rate_limit");
        let rate_limit = &strategy.middleware[0];
        assert_eq!(rate_limit.name(), "rate_limit");
        let config = rate_limit.config_as_json().unwrap();
        assert_eq!(config["requests"], 100);
    }

    #[test]
    fn test_supplement_middleware_yaml_formats() {
        let mut current = vec![
            MiddlewareConfig::new_named_yaml(
                "cache".to_string(),
                serde_yaml::from_str(r#"
ttl: 300
max_size: 1000
"#).unwrap()
            )
        ];

        let incoming = vec![
            MiddlewareConfig::new_named_json(
                "cache".to_string(),
                json!({
                    "ttl": 600,        // Should NOT be updated (exists)
                    "strategy": "lru"  // Should be added (missing)
                })
            )
        ];

        supplement_middleware(&mut current, &incoming).unwrap();

        let cache = &current[0];
        assert_eq!(cache.name(), "cache");
        let config = cache.config_as_json().unwrap();
        assert_eq!(config["ttl"], 300);        // NOT updated
        assert_eq!(config["max_size"], 1000);  // Preserved
        assert_eq!(config["strategy"], "lru"); // Added (was missing)
    }

    #[test]
    fn test_supplement_middleware_no_overwrite() {
        let mut current = vec![
            MiddlewareConfig::new_named_json(
                "test".to_string(),
                json!({
                    "existing_string": "old_value",
                    "existing_number": 42,
                    "existing_bool": true,
                    "existing_null": null,
                    "existing_array": [1, 2, 3],
                    "existing_object": {"key": "value"}
                })
            )
        ];

        let incoming = vec![
            MiddlewareConfig::new_named_json(
                "test".to_string(),
                json!({
                    "existing_string": "new_value",    // Should NOT be updated
                    "existing_number": 100,            // Should NOT be updated
                    "existing_bool": false,             // Should NOT be updated
                    "existing_null": "not_null",       // Should NOT be updated (null exists)
                    "existing_array": [4, 5, 6],        // Should NOT be updated
                    "existing_object": {"new_key": "new_value"}, // Should NOT be updated
                    "new_string": "added",             // Should be added
                    "new_number": 999,                 // Should be added
                    "new_bool": false,                 // Should be added
                    "new_array": [7, 8, 9],            // Should be added
                    "new_object": {"added": "yes"}     // Should be added
                })
            )
        ];

        supplement_middleware(&mut current, &incoming).unwrap();

        let test = &current[0];
        let config = test.config_as_json().unwrap();

        // Check existing values are NOT updated
        assert_eq!(config["existing_string"], "old_value");
        assert_eq!(config["existing_number"], 42);
        assert_eq!(config["existing_bool"], true);
        assert_eq!(config["existing_null"], serde_json::Value::Null);
        assert_eq!(config["existing_array"], json!([1, 2, 3]));
        assert_eq!(config["existing_object"], json!({"key": "value", "new_key": "new_value"}));

        // Check new values are added
        assert_eq!(config["new_string"], "added");
        assert_eq!(config["new_number"], 999);
        assert_eq!(config["new_bool"], false);
        assert_eq!(config["new_array"], json!([7, 8, 9]));
        assert_eq!(config["new_object"], json!({"added": "yes"}));
    }

    #[test]
    fn test_inline_middleware_strategy_ref() {
        let yaml_str = r#"
    - rate_limit:
        requests: 100
        window: "1m"
    - logging:
        level: debug
    "#;
        
        let strategy_ref: StrategyRef = serde_yaml::from_str(yaml_str).unwrap();
        
        match strategy_ref {
            StrategyRef::InlineMiddleware(middleware) => {
                assert_eq!(middleware.len(), 2);
                assert_eq!(middleware[0].name(), "rate_limit");
                assert_eq!(middleware[1].name(), "logging");
            }
            _ => panic!("Expected InlineMiddleware variant"),
        }
    }

    #[test]
    fn test_inline_middleware_strategy_ref_resolution() {
        let yaml_str = r#"
    - rate_limit:
        requests: 50
        window: "30s"
    - logging:
        level: info
    "#;
        
        let strategy_ref: StrategyRef = serde_yaml::from_str(yaml_str).unwrap();
        let strategies = LegacyStrategyCollection::new();
        
        let resolved = strategy_ref.resolve(&strategies).unwrap();
        assert_eq!(resolved.name, "inline");
        assert_eq!(resolved.middleware.len(), 2);
        assert_eq!(resolved.middleware[0].name(), "rate_limit");
        assert_eq!(resolved.middleware[1].name(), "logging");
    }

    #[test]
    fn test_from_yaml_str_convenience() {
        let middleware = MiddlewareConfig::from_yaml_str(
            "httpward_log_module", 
            "level: warn"
        ).unwrap();

        assert_eq!(middleware.name(), "httpward_log_module");
        assert!(!middleware.is_off());

        let config: serde_json::Value = middleware.config_as_json().unwrap();
        assert_eq!(config["level"], "warn");
    }

    #[test]
    fn test_from_json_str_convenience() {
        let middleware = MiddlewareConfig::from_json_str(
            "httpward_log_module", 
            r#"{"level": "info", "tag": "api"}"#
        ).unwrap();

        assert_eq!(middleware.name(), "httpward_log_module");
        assert!(!middleware.is_off());

        let config: serde_json::Value = middleware.config_as_json().unwrap();
        assert_eq!(config["level"], "info");
        assert_eq!(config["tag"], "api");
    }

    #[test]
    fn test_from_serializable_convenience() {
        #[derive(Serialize)]
        struct LogConfig {
            level: String,
            tag: Option<String>,
        }

        let config = LogConfig {
            level: "debug".to_string(),
            tag: Some("test".to_string()),
        };

        let middleware = MiddlewareConfig::from_serializable(
            "httpward_log_module", 
            config
        ).unwrap();

        assert_eq!(middleware.name(), "httpward_log_module");
        assert!(!middleware.is_off());

        let parsed_config: serde_json::Value = middleware.config_as_json().unwrap();
        assert_eq!(parsed_config["level"], "debug");
        assert_eq!(parsed_config["tag"], "test");
    }

    #[test]
    fn test_parse_config_convenience() {
        #[derive(Deserialize, PartialEq, Debug)]
        struct LogConfig {
            level: String,
            tag: Option<String>,
        }

        let middleware = MiddlewareConfig::from_yaml_str(
            "httpward_log_module", 
            "level: warn\ntag: test"
        ).unwrap();

        let config: LogConfig = middleware.parse_config().unwrap();
        assert_eq!(config.level, "warn");
        assert_eq!(config.tag, Some("test".to_string()));
    }

    #[test]
    fn test_yaml_round_trip_convenience() {
        let original = MiddlewareConfig::from_yaml_str(
            "test_middleware", 
            "level: warn\ntag: test"
        ).unwrap();

        let yaml_string = original.config_to_yaml_string().unwrap();
        let restored = MiddlewareConfig::from_yaml_str("test_middleware", &yaml_string).unwrap();

        let original_config: serde_json::Value = original.config_as_json().unwrap();
        let restored_config: serde_json::Value = restored.config_as_json().unwrap();
        
        assert_eq!(original_config, restored_config);
    }

    #[test]
    fn test_json_round_trip_convenience() {
        let original = MiddlewareConfig::from_json_str(
            "test_middleware", 
            r#"{"level": "warn", "tag": "test"}"#
        ).unwrap();

        let json_string = original.config_to_json_string().unwrap();
        let restored = MiddlewareConfig::from_json_str("test_middleware", &json_string).unwrap();

        let original_config: serde_json::Value = original.config_as_json().unwrap();
        let restored_config: serde_json::Value = restored.config_as_json().unwrap();
        
        assert_eq!(original_config, restored_config);
    }
}

// Include the off functionality tests
mod off_tests;
