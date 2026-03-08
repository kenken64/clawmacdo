//! Core types, configuration, and error handling for ClawMacdo

pub mod config;
pub mod error;

// Re-export commonly used items
pub use config::*;
pub use error::*;