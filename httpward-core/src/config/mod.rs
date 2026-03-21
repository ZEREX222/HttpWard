// src/config/mod.rs
// This is the "facade" of the config module - everything else will be imported from here.

pub mod global;
mod loader;
mod site;
pub mod strategy;

pub use global::{GlobalConfig, Listener, LogConfig, Match, Redirect, Route, Tls};
pub use loader::{AppConfig, load};
pub use site::SiteConfig;
pub use strategy::{
    LegacyStrategyCollection as StrategyCollection, MiddlewareConfig, Strategy, StrategyRef,
    UniversalValue,
};
