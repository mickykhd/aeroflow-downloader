//! AeroFlow core engine — pure Rust, no Tauri dependency.
//! Covers: task model, multi-segment downloader, token-bucket throttle,
//! weekly scheduler matrix, SHA-256 verification, proxy pool, JSON store.

pub mod downloader;
pub mod hash;
pub mod model;
pub mod proxy;
pub mod scheduler;
pub mod store;
pub mod throttle;

pub use model::*;
