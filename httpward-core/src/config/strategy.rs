use serde::{Deserialize, Serialize, Deserializer};
use std::collections::HashMap;

pub type StrategyCollection = HashMap<String, Vec<MiddlewareConfig>>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Strategy {
    pub name: String,

    #[serde(default)]
    pub middleware: Vec<MiddlewareConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub enum MiddlewareConfig {
    Named {
        name: String,
        config: serde_yaml::Value,
    },
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

        let (name, config) = map.into_iter().next().unwrap();

        Ok(MiddlewareConfig::Named { name, config })
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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
