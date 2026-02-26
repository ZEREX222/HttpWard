// src/config/mod.rs
// Это "фасад" модуля config — отсюда будут импортировать всё остальное

mod global;
mod site;
mod loader;

pub use global::{GlobalConfig, TlsDefault, LogConfig};
pub use site::{SiteConfig, TlsOverride, Route, Match, Redirect};
pub use loader::{AppConfig};
