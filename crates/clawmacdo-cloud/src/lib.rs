//! Cloud provider implementations for ClawMacdo

pub mod cloud_init;
pub mod cloud_provider;

#[cfg(feature = "digitalocean")]
pub mod digitalocean;

#[cfg(feature = "lightsail")]
pub mod lightsail_cli;

#[cfg(feature = "tencent")]
pub mod tencent;

// Re-export main types and traits
pub use cloud_init::*;
pub use cloud_provider::*;
