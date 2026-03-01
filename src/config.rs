use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_REGION: &str = "sgp1";
pub const DEFAULT_SIZE: &str = "s-2vcpu-4gb";
pub const OPENCLAW_GATEWAY_PORT: u16 = 18789;
pub const DROPLET_TAG: &str = "openclaw";
pub const CLOUD_INIT_SENTINEL: &str = "/root/.clawmacdo_cloud_init_done";
pub const OPENCLAW_USER: &str = "openclaw";
pub const OPENCLAW_HOME: &str = "/home/openclaw";

/// Resolve the app data directory: ~/.clawmacdo/
pub fn app_dir() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or(AppError::HomeDirNotFound)?;
    Ok(home.join(".clawmacdo"))
}

/// ~/.clawmacdo/backups/
pub fn backups_dir() -> Result<PathBuf, AppError> {
    Ok(app_dir()?.join("backups"))
}

/// ~/.clawmacdo/keys/
pub fn keys_dir() -> Result<PathBuf, AppError> {
    Ok(app_dir()?.join("keys"))
}

/// ~/.clawmacdo/deploys/
pub fn deploys_dir() -> Result<PathBuf, AppError> {
    Ok(app_dir()?.join("deploys"))
}

/// ~/.openclaw/
pub fn openclaw_dir() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or(AppError::HomeDirNotFound)?;
    Ok(home.join(".openclaw"))
}

/// macOS LaunchAgent plist path
pub fn launchagent_plist() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or(AppError::HomeDirNotFound)?;
    Ok(home.join("Library/LaunchAgents/ai.openclaw.gateway.plist"))
}

/// Ensure all app directories exist
pub fn ensure_dirs() -> Result<(), AppError> {
    std::fs::create_dir_all(backups_dir()?)?;
    std::fs::create_dir_all(keys_dir()?)?;
    std::fs::create_dir_all(deploys_dir()?)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeployRecord {
    pub id: String,
    pub droplet_id: u64,
    pub hostname: String,
    pub ip_address: String,
    pub region: String,
    pub size: String,
    pub ssh_key_path: String,
    pub ssh_key_fingerprint: String,
    pub backup_restored: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl DeployRecord {
    pub fn save(&self) -> Result<PathBuf, AppError> {
        let path = deploys_dir()?.join(format!("{}.json", self.id));
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(path)
    }
}
