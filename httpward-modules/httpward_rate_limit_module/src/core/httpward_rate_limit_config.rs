use super::RateLimitKeyKind;
/// User-facing YAML configuration format for rate limiting.
///
/// This is the primary configuration format for HttpWardRateLimitLayer.
///
/// Example:
/// ```yaml
/// httpward_rate_limit_module:
///   global_config:
///     max_entries: 100_000
///     idle_ttl_sec: 60
///     cleanup_interval_sec: 10
///   global_rules:
///     - ip:
///         max_requests: 100
///         window: 10s
///     - ja4:
///         max_requests: 150
///         window: 10s
///   current_site_rules:
///     - ip:
///         max_requests: 50
///         window: 10s
/// ```
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Main rate limit configuration (YAML format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct HttpWardRateLimitConfig {
    /// Global store configuration.
    pub global_config: Option<RateLimitStoreConfig>,
    /// Global rate limit rules (apply to all routes).
    pub global_rules: Vec<HashMap<String, RateLimitRuleConfig>>,
    /// Current route/site rate limit rules.
    #[serde(alias = "route_rules")]
    pub current_site_rules: Vec<HashMap<String, RateLimitRuleConfig>>,
    /// Response settings.
    pub response: Option<RateLimitResponseConfig>,
}


impl HttpWardRateLimitConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_rules(&self) -> bool {
        !(self.global_rules.is_empty() && self.current_site_rules.is_empty())
    }

    /// Convert to internal rate limit structure.
    pub fn to_internal(&self) -> InternalRateLimitConfig {
        let store = self
            .global_config
            .as_ref()
            .map(|cfg| cfg.to_internal())
            .unwrap_or_default();

        let global = self
            .global_rules
            .iter()
            .flat_map(|rule_map| {
                rule_map
                    .iter()
                    .filter_map(|(key_str, rule_cfg)| {
                        let key = parse_rate_limit_key(key_str)?;
                        Some(InternalRateLimitRule {
                            key,
                            capacity: rule_cfg.max_requests,
                            refill_every: rule_cfg.window_duration(),
                            refill_amount: 1, // Will be overridden by strategy logic
                            strategy: rule_cfg.strategy.clone(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let matched_route = self
            .current_site_rules
            .iter()
            .flat_map(|rule_map| {
                rule_map
                    .iter()
                    .filter_map(|(key_str, rule_cfg)| {
                        let key = parse_rate_limit_key(key_str)?;
                        Some(InternalRateLimitRule {
                            key,
                            capacity: rule_cfg.max_requests,
                            refill_every: rule_cfg.window_duration(),
                            refill_amount: 1, // Will be overridden by strategy logic
                            strategy: rule_cfg.strategy.clone(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let response = self
            .response
            .as_ref()
            .map(|r| r.to_internal())
            .unwrap_or_default();

        InternalRateLimitConfig {
            store,
            global,
            matched_route,
            response,
        }
    }
}

/// Store configuration in YAML format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct RateLimitStoreConfig {
    /// Maximum entries in memory.
    pub max_entries: Option<usize>,
    /// Idle TTL in seconds.
    pub idle_ttl_sec: Option<u64>,
    /// Cleanup interval in seconds.
    pub cleanup_interval_sec: Option<u64>,
}


impl RateLimitStoreConfig {
    fn to_internal(&self) -> InternalRateLimitStoreConfig {
        InternalRateLimitStoreConfig {
            max_entries: self.max_entries.unwrap_or(100_000),
            idle_ttl_secs: self.idle_ttl_sec.unwrap_or(60),
            cleanup_interval_secs: self.cleanup_interval_sec.unwrap_or(10),
        }
    }
}

/// Rate limit strategy types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum RateLimitStrategy {
    /// Sliding window - gradual token refill
    #[default]
    Sliding,
    /// Burst protection - large capacity, slow refill
    Burst,
    /// Fixed window - complete refill each period
    Fixed,
}


/// Rate limit rule in YAML format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitRuleConfig {
    /// Maximum requests in the window.
    pub max_requests: u32,
    /// Time window as a duration string (e.g., "10s", "1m").
    #[serde(default = "default_window")]
    pub window: String,
    /// Rate limiting strategy.
    #[serde(default)]
    pub strategy: RateLimitStrategy,
}

fn default_window() -> String {
    "10s".to_string()
}

impl Default for RateLimitRuleConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window: "10s".to_string(),
            strategy: RateLimitStrategy::Sliding,
        }
    }
}

impl RateLimitRuleConfig {
    /// Parse the `window` string into a [`Duration`].
    pub fn window_duration(&self) -> Duration {
        parse_duration(&self.window).unwrap_or(Duration::from_secs(10))
    }

    /// Millisecond representation of the window (kept for compatibility).
    pub fn window_ms(&self) -> u64 {
        self.window_duration().as_millis() as u64
    }
}

/// Response configuration in YAML format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitResponseConfig {
    /// HTTP status code.
    pub status_code: Option<u16>,
    /// Response body.
    pub body: Option<String>,
}

impl Default for RateLimitResponseConfig {
    fn default() -> Self {
        Self {
            status_code: Some(429),
            body: Some("Rate limit exceeded".to_string()),
        }
    }
}

impl RateLimitResponseConfig {
    fn to_internal(&self) -> InternalRateLimitResponseConfig {
        InternalRateLimitResponseConfig {
            status_code: self.status_code.unwrap_or(429),
            body: self
                .body.clone()
                .unwrap_or_else(|| "Rate limit exceeded".to_string()),
        }
    }
}

// ============= Internal Config Structures =============

/// Internal store configuration (after conversion from YAML).
#[derive(Debug, Clone)]
pub struct InternalRateLimitStoreConfig {
    pub max_entries: usize,
    pub idle_ttl_secs: u64,
    pub cleanup_interval_secs: u64,
}

impl Default for InternalRateLimitStoreConfig {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            idle_ttl_secs: 60,
            cleanup_interval_secs: 10,
        }
    }
}

/// Internal rate limit rule (after conversion from YAML).
#[derive(Debug, Clone)]
pub struct InternalRateLimitRule {
    pub key: RateLimitKeyKind,
    pub capacity: u32,
    /// Pre-parsed refill window.
    pub refill_every: Duration,
    pub refill_amount: u32,
    pub strategy: RateLimitStrategy,
}

impl InternalRateLimitRule {
    pub fn to_runtime_rule(&self) -> super::RateLimitRule {
        match self.strategy {
            RateLimitStrategy::Sliding => {
                // Gradual refill: window / capacity = refill interval
                let refill_interval = self
                    .refill_every
                    .checked_div(self.capacity.max(1))
                    .unwrap_or(self.refill_every);
                super::RateLimitRule {
                    capacity: self.capacity.max(1),
                    refill_every: if refill_interval.is_zero() {
                        Duration::from_millis(1)
                    } else {
                        refill_interval
                    },
                    refill_amount: 1,
                }
            }
            RateLimitStrategy::Burst => {
                // Current behavior: large capacity, slow refill
                super::RateLimitRule {
                    capacity: self.capacity.max(1),
                    refill_every: if self.refill_every.is_zero() {
                        Duration::from_millis(1)
                    } else {
                        self.refill_every
                    },
                    refill_amount: 1,
                }
            }
            RateLimitStrategy::Fixed => {
                // Fixed window: complete refill each period
                super::RateLimitRule {
                    capacity: self.capacity.max(1),
                    refill_every: if self.refill_every.is_zero() {
                        Duration::from_millis(1)
                    } else {
                        self.refill_every
                    },
                    refill_amount: self.capacity.max(1),
                }
            }
        }
    }
}

/// Internal response configuration (after conversion from YAML).
#[derive(Debug, Clone)]
pub struct InternalRateLimitResponseConfig {
    pub status_code: u16,
    pub body: String,
}

impl Default for InternalRateLimitResponseConfig {
    fn default() -> Self {
        Self {
            status_code: 429,
            body: "Rate limit exceeded".to_string(),
        }
    }
}

/// Internal rate limit configuration (after full conversion from YAML).
#[derive(Debug, Clone)]
pub struct InternalRateLimitConfig {
    pub store: InternalRateLimitStoreConfig,
    pub global: Vec<InternalRateLimitRule>,
    pub matched_route: Vec<InternalRateLimitRule>,
    pub response: InternalRateLimitResponseConfig,
}

// ============= Helper Functions =============

fn parse_rate_limit_key(s: &str) -> Option<RateLimitKeyKind> {
    match s.to_lowercase().as_str() {
        "ip" => Some(RateLimitKeyKind::Ip),
        "ja4" => Some(RateLimitKeyKind::Ja4),
        "header" | "header_fingerprint" => Some(RateLimitKeyKind::HeaderFingerprint),
        "cookie" => Some(RateLimitKeyKind::Cookie),
        _ => None,
    }
}

/// Parse a human-friendly duration string into a [`Duration`].
///
/// Supported formats:
/// - bare integer → treated as seconds (`"10"` → 10 s)
/// - `"100ms"` / `"milliseconds"` → milliseconds
/// - `"10s"` / `"seconds"` → seconds
/// - `"5m"` / `"minutes"` → minutes
/// - `"2h"` / `"hours"` → hours
///
/// Returns `None` if the string cannot be parsed.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();

    // Bare integer → seconds
    if let Ok(secs) = s.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }

    let mut num_str = String::new();
    let mut unit_str = String::new();
    let mut in_unit = false;

    for ch in s.chars() {
        if ch.is_numeric() || ch == '.' {
            if in_unit {
                return None; // digit after unit
            }
            num_str.push(ch);
        } else if ch.is_alphabetic() {
            in_unit = true;
            unit_str.push(ch);
        } else if ch.is_whitespace() {
            continue;
        } else {
            return None;
        }
    }

    let num = num_str.parse::<f64>().ok()?;

    let duration = match unit_str.to_lowercase().as_str() {
        "ms" | "millisecond" | "milliseconds" => Duration::from_millis(num as u64),
        "s" | "sec" | "second" | "seconds" => Duration::from_millis((num * 1_000.0) as u64),
        "m" | "min" | "minute" | "minutes" => Duration::from_millis((num * 60_000.0) as u64),
        "h" | "hour" | "hours" => Duration::from_millis((num * 3_600_000.0) as u64),
        _ => return None,
    };

    Some(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("10s"), Some(Duration::from_secs(10)));
        assert_eq!(parse_duration("1m"), Some(Duration::from_secs(60)));
        assert_eq!(parse_duration("100ms"), Some(Duration::from_millis(100)));
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7_200)));
        assert_eq!(parse_duration("30"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("unknown"), None);
        // ms-value sanity checks
        assert_eq!(parse_duration("10s").map(|d| d.as_millis()), Some(10_000));
        assert_eq!(parse_duration("1m").map(|d| d.as_millis()), Some(60_000));
        assert_eq!(parse_duration("100ms").map(|d| d.as_millis()), Some(100));
        assert_eq!(parse_duration("2h").map(|d| d.as_millis()), Some(7_200_000));
    }

    #[test]
    fn test_parse_rate_limit_key() {
        assert_eq!(parse_rate_limit_key("ip"), Some(RateLimitKeyKind::Ip));
        assert_eq!(parse_rate_limit_key("ja4"), Some(RateLimitKeyKind::Ja4));
        assert_eq!(
            parse_rate_limit_key("header"),
            Some(RateLimitKeyKind::HeaderFingerprint)
        );
        assert_eq!(
            parse_rate_limit_key("cookie"),
            Some(RateLimitKeyKind::Cookie)
        );
    }

    #[test]
    fn test_config_to_internal() {
        let mut global_rules = HashMap::new();
        global_rules.insert(
            "ip".to_string(),
            RateLimitRuleConfig {
                max_requests: 100,
                window: "10s".to_string(),
                strategy: RateLimitStrategy::Sliding,
            },
        );

        let config = HttpWardRateLimitConfig {
            global_config: Some(RateLimitStoreConfig {
                max_entries: Some(50_000),
                idle_ttl_sec: Some(30),
                cleanup_interval_sec: Some(5),
            }),
            global_rules: vec![global_rules],
            current_site_rules: Vec::new(),
            response: None,
        };

        let internal = config.to_internal();
        assert_eq!(internal.store.max_entries, 50_000);
        assert_eq!(internal.store.idle_ttl_secs, 30);
        assert_eq!(internal.global.len(), 1);
        assert_eq!(internal.global[0].strategy, RateLimitStrategy::Sliding);
    }

    #[test]
    fn test_sliding_strategy_conversion() {
        let rule = InternalRateLimitRule {
            key: RateLimitKeyKind::Ip,
            capacity: 50,
            refill_every: Duration::from_secs(10),
            refill_amount: 1,
            strategy: RateLimitStrategy::Sliding,
        };

        let runtime_rule = rule.to_runtime_rule();

        // Sliding: 10s / 50 = 200ms per token
        assert_eq!(runtime_rule.capacity, 50);
        assert_eq!(runtime_rule.refill_every, Duration::from_millis(200));
        assert_eq!(runtime_rule.refill_amount, 1);
    }

    #[test]
    fn test_burst_strategy_conversion() {
        let rule = InternalRateLimitRule {
            key: RateLimitKeyKind::Ip,
            capacity: 50,
            refill_every: Duration::from_secs(10),
            refill_amount: 1,
            strategy: RateLimitStrategy::Burst,
        };

        let runtime_rule = rule.to_runtime_rule();

        // Burst: original parameters preserved
        assert_eq!(runtime_rule.capacity, 50);
        assert_eq!(runtime_rule.refill_every, Duration::from_secs(10));
        assert_eq!(runtime_rule.refill_amount, 1);
    }

    #[test]
    fn test_fixed_strategy_conversion() {
        let rule = InternalRateLimitRule {
            key: RateLimitKeyKind::Ip,
            capacity: 50,
            refill_every: Duration::from_secs(10),
            refill_amount: 1,
            strategy: RateLimitStrategy::Fixed,
        };

        let runtime_rule = rule.to_runtime_rule();

        // Fixed: complete refill each period
        assert_eq!(runtime_rule.capacity, 50);
        assert_eq!(runtime_rule.refill_every, Duration::from_secs(10));
        assert_eq!(runtime_rule.refill_amount, 50);
    }

    #[test]
    fn test_strategy_edge_cases() {
        // Test zero capacity
        let rule = InternalRateLimitRule {
            key: RateLimitKeyKind::Ip,
            capacity: 0,
            refill_every: Duration::from_secs(10),
            refill_amount: 1,
            strategy: RateLimitStrategy::Sliding,
        };

        let runtime_rule = rule.to_runtime_rule();
        assert_eq!(runtime_rule.capacity, 1); // min(1)
        assert_eq!(runtime_rule.refill_every, Duration::from_secs(10)); // division by 0 avoided
    }

    #[test]
    fn test_full_config_with_all_strategies() {
        let mut global_rules = HashMap::new();
        global_rules.insert(
            "ip".to_string(),
            RateLimitRuleConfig {
                max_requests: 50,
                window: "10s".to_string(),
                strategy: RateLimitStrategy::Sliding,
            },
        );

        let mut site_rules = HashMap::new();
        site_rules.insert(
            "ja4".to_string(),
            RateLimitRuleConfig {
                max_requests: 1000,
                window: "300s".to_string(),
                strategy: RateLimitStrategy::Burst,
            },
        );

        let config = HttpWardRateLimitConfig {
            global_config: None,
            global_rules: vec![global_rules],
            current_site_rules: vec![site_rules],
            response: None,
        };

        let internal = config.to_internal();

        // Check global sliding rule
        assert_eq!(internal.global.len(), 1);
        assert_eq!(internal.global[0].strategy, RateLimitStrategy::Sliding);
        assert_eq!(internal.global[0].capacity, 50);

        // Check site burst rule
        assert_eq!(internal.matched_route.len(), 1);
        assert_eq!(internal.matched_route[0].strategy, RateLimitStrategy::Burst);
        assert_eq!(internal.matched_route[0].capacity, 1000);
    }

    #[test]
    fn test_rate_limit_strategy_default() {
        assert_eq!(RateLimitStrategy::default(), RateLimitStrategy::Sliding);
    }

    #[test]
    fn test_rate_limit_rule_config_default() {
        let default = RateLimitRuleConfig::default();
        assert_eq!(default.max_requests, 100);
        assert_eq!(default.window, "10s");
        assert_eq!(default.strategy, RateLimitStrategy::Sliding);
    }
}
