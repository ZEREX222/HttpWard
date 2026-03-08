// src/config/mod.rs
// This is the "facade" of the config module - everything else will be imported from here.

mod global;
mod loader;
mod site;
mod strategy;

pub use global::{GlobalConfig, Listener, Route, Match, Redirect, Tls, LogConfig};
pub use loader::{AppConfig, load};
pub use site::SiteConfig;
pub use strategy::{
    Strategy, StrategyCollection, StrategyRef,
    MiddlewareConfig
};
