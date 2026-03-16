use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_REGION: &str = "sgp1";
pub const DEFAULT_SIZE: &str = "s-2vcpu-4gb";

// Tencent Cloud defaults
pub const DEFAULT_TENCENT_REGION: &str = "ap-singapore";
pub const DEFAULT_TENCENT_INSTANCE_TYPE: &str = "SA5.MEDIUM4";
pub const DEFAULT_TENCENT_IMAGE_ID: &str = "img-487zeit5"; // Ubuntu 24.04 LTS

// Azure defaults
pub const DEFAULT_AZURE_REGION: &str = "southeastasia";
pub const DEFAULT_AZURE_SIZE: &str = "Standard_B2s";
pub const DEFAULT_AZURE_IMAGE: &str = "Canonical:ubuntu-24_04-lts:server:latest";

// BytePlus defaults
pub const DEFAULT_BYTEPLUS_REGION: &str = "ap-southeast-1";
pub const DEFAULT_BYTEPLUS_SIZE: &str = "ecs.g3i.large";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CloudProviderType {
    DigitalOcean,
    Tencent,
    Lightsail,
    Azure,
    BytePlus,
}

impl std::fmt::Display for CloudProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudProviderType::DigitalOcean => write!(f, "digitalocean"),
            CloudProviderType::Tencent => write!(f, "tencent"),
            CloudProviderType::Lightsail => write!(f, "lightsail"),
            CloudProviderType::Azure => write!(f, "azure"),
            CloudProviderType::BytePlus => write!(f, "byteplus"),
        }
    }
}
pub const OPENCLAW_GATEWAY_PORT: u16 = 18789;
pub const DROPLET_TAG: &str = "openclaw";
pub const CLOUD_INIT_SENTINEL: &str = "/root/.clawmacdo_cloud_init_done";
pub const OPENCLAW_USER: &str = "openclaw";
pub const OPENCLAW_HOME: &str = "/home/openclaw";

/// Resolve the app data directory: ~/.clawmacdo/
/// AApp dir.
pub fn app_dir() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or(AppError::HomeDirNotFound)?;
    Ok(home.join(".clawmacdo"))
}

/// ~/.clawmacdo/backups/
/// BBackups dir.
pub fn backups_dir() -> Result<PathBuf, AppError> {
    Ok(app_dir()?.join("backups"))
}

/// ~/.clawmacdo/keys/
/// KKeys dir.
pub fn keys_dir() -> Result<PathBuf, AppError> {
    Ok(app_dir()?.join("keys"))
}

/// ~/.clawmacdo/known_hosts
pub fn known_hosts_path() -> Result<PathBuf, AppError> {
    Ok(app_dir()?.join("known_hosts"))
}

/// ~/.clawmacdo/deploys/
/// DDeploys dir.
pub fn deploys_dir() -> Result<PathBuf, AppError> {
    Ok(app_dir()?.join("deploys"))
}

/// ~/.openclaw/
/// OOpenclaw dir.
pub fn openclaw_dir() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or(AppError::HomeDirNotFound)?;
    Ok(home.join(".openclaw"))
}

/// macOS LaunchAgent plist path
/// LLaunchagent plist.
pub fn launchagent_plist() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or(AppError::HomeDirNotFound)?;
    Ok(home.join("Library/LaunchAgents/ai.openclaw.gateway.plist"))
}

/// Ensure all app directories exist
/// EEnsure dirs.
pub fn ensure_dirs() -> Result<(), AppError> {
    std::fs::create_dir_all(backups_dir()?)?;
    std::fs::create_dir_all(keys_dir()?)?;
    std::fs::create_dir_all(deploys_dir()?)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeployRecord {
    pub id: String,
    #[serde(default)]
    pub provider: Option<CloudProviderType>,
    pub droplet_id: u64,
    /// For Tencent, this stores the instance ID string (droplet_id will be 0).
    #[serde(default)]
    pub instance_id: Option<String>,
    pub hostname: String,
    pub ip_address: String,
    pub region: String,
    pub size: String,
    pub ssh_key_path: String,
    pub ssh_key_fingerprint: String,
    /// For Tencent, stores the KeyPair ID for cleanup.
    #[serde(default)]
    pub ssh_key_id: Option<String>,
    /// For Azure, stores the resource group name for cleanup.
    #[serde(default)]
    pub resource_group: Option<String>,
    pub backup_restored: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl DeployRecord {
    /// SSave.
    pub fn save(&self) -> Result<PathBuf, AppError> {
        let path = deploys_dir()?.join(format!("{}.json", self.id));
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(path)
    }
}
