//! Cloud provider implementations for ClawMacdo

pub mod cloud_provider;
pub mod cloud_init;
pub mod digitalocean;
pub mod tencent;

// Re-export main types and traits
pub use cloud_provider::*;
pub use cloud_init::*;