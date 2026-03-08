//! SSH/SCP operations and key management for ClawMacdo

pub mod ssh;

// Re-export main functionality
pub use ssh::*;