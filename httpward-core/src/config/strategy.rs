use serde::{Deserialize, Serialize, Deserializer};
use std::collections::HashMap;
use anyhow::Result;
use schemars::JsonSchema;

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

pub type StrategyCollection = HashMap<String, Vec<MiddlewareConfig>>;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Strategy {
    pub name: String,

    #[serde(default)]
    pub middleware: Vec<MiddlewareConfig>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub enum MiddlewareConfig {
    Named {
        name: String,
        #[schemars(with = "serde_json::Value")]
        config: UniversalValue,
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
    
    /// Get middleware name
    pub fn name(&self) -> &str {
        match self {
            MiddlewareConfig::Named { name, .. } => name,
        }
    }
    
    /// Get configuration as JSON Value
    pub fn config_as_json(&self) -> Result<serde_json::Value> {
        match self {
            MiddlewareConfig::Named { config, .. } => config.as_json(),
        }
    }
    
    /// Get configuration as YAML Value
    pub fn config_as_yaml(&self) -> Result<serde_yaml::Value> {
        match self {
            MiddlewareConfig::Named { config, .. } => config.as_yaml(),
        }
    }
    
    /// Get configuration as JSON string
    pub fn config_to_json_string(&self) -> Result<String> {
        match self {
            MiddlewareConfig::Named { config, .. } => config.to_json_string(),
        }
    }
    
    /// Get configuration as YAML string
    pub fn config_to_yaml_string(&self) -> Result<String> {
        match self {
            MiddlewareConfig::Named { config, .. } => config.to_yaml_string(),
        }
    }
    
    /// Convert configuration to specific type using serde
    pub fn config_into<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        let json_val = self.config_as_json()?;
        serde_json::from_value(json_val).map_err(Into::into)
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

        Ok(MiddlewareConfig::Named { 
            name, 
            config: UniversalValue::from_yaml(yaml_value)
        })
    }
}

impl Strategy {
    pub fn new(name: String) -> Self {
        Self {
            name,
            middleware: Vec::new(),
        }
    }

    pub fn merge_with(&self, other: &Strategy) -> Strategy {
        let mut result = self.clone();

        for middleware in &other.middleware {
            match middleware {
                MiddlewareConfig::Named { name, .. } => {
                    if let Some(pos) = result.middleware.iter().position(|m| {
                        matches!(
                            m,
                            MiddlewareConfig::Named { name: existing, .. }
                            if existing == name
                        )
                    }) {
                        result.middleware[pos] = middleware.clone();
                    } else {
                        result.middleware.push(middleware.clone());
                    }
                }
            }
        }

        result
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum StrategyRef {
    Named(String),
    Inline(Strategy),
}

impl StrategyRef {
    pub fn resolve(&self, strategies: &StrategyCollection) -> Option<Strategy> {
        match self {
            StrategyRef::Named(name) => strategies.get(name).map(|middleware| Strategy {
                name: name.clone(),
                middleware: middleware.clone(),
            }),
            StrategyRef::Inline(strategy) => Some(strategy.clone()),
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
}
