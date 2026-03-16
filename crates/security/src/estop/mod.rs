//! Emergency stop system
//!
//! Provides 4-level emergency stop with JSON state persistence.

pub mod manager;
pub mod types;

pub use manager::EstopManager;
pub use types::{EstopLevel, EstopState};
