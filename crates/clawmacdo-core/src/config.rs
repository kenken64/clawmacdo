use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
pub const STATE_DIR_ENV: &str = "CLAWMACDO_STATE_DIR";
pub const RAILWAY_VOLUME_MOUNT_PATH_ENV: &str = "RAILWAY_VOLUME_MOUNT_PATH";

fn explicit_state_dir(value: Option<&str>, label: &str) -> Result<Option<PathBuf>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let path = if trimmed == "~" {
        dirs::home_dir().ok_or(AppError::HomeDirNotFound)?
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        dirs::home_dir()
            .ok_or(AppError::HomeDirNotFound)?
            .join(rest)
    } else {
        PathBuf::from(trimmed)
    };

    if !path.is_absolute() {
        return Err(AppError::Generic(format!(
            "{label} must be an absolute path"
        )));
    }

    Ok(Some(path))
}

/// Resolve the app data directory.
///
/// Resolution order:
/// 1. `CLAWMACDO_STATE_DIR`
/// 2. Railway's mounted volume path, when mounted as `.clawmacdo`
/// 3. `~/.clawmacdo`
pub fn app_dir() -> Result<PathBuf, AppError> {
    let state_dir = std::env::var(STATE_DIR_ENV).ok();
    if let Some(path) = explicit_state_dir(state_dir.as_deref(), STATE_DIR_ENV)? {
        return Ok(path);
    }

    let railway_volume_mount = std::env::var(RAILWAY_VOLUME_MOUNT_PATH_ENV).ok();
    if let Some(path) = explicit_state_dir(
        railway_volume_mount.as_deref(),
        RAILWAY_VOLUME_MOUNT_PATH_ENV,
    )? {
        if path.file_name().and_then(|name| name.to_str()) == Some(".clawmacdo") {
            return Ok(path);
        }
    }

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

fn canonicalize_scoped_existing_path(
    input: &str,
    base_dir: &Path,
    label: &str,
) -> Result<PathBuf, AppError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::Generic(format!("{label} path is required")));
    }

    let canonical = std::fs::canonicalize(trimmed)
        .map_err(|e| AppError::Generic(format!("Invalid {label} path: {e}")))?;
    let scoped_base = std::fs::canonicalize(base_dir)
        .map_err(|e| AppError::Generic(format!("Invalid {label} directory: {e}")))?;

    if !canonical.starts_with(&scoped_base) {
        return Err(AppError::Generic(format!(
            "{label} path must stay within {}",
            scoped_base.display()
        )));
    }

    Ok(canonical)
}

pub fn resolve_backup_path(input: &str) -> Result<PathBuf, AppError> {
    let base = backups_dir()?;
    canonicalize_scoped_existing_path(input, &base, "backup")
}

pub fn resolve_key_path(input: &str) -> Result<PathBuf, AppError> {
    let base = keys_dir()?;
    canonicalize_scoped_existing_path(input, &base, "SSH key")
}

pub fn normalize_hostname(input: &str) -> Result<Option<String>, AppError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let hostname = trimmed.to_ascii_lowercase();
    if hostname.len() > 253 {
        return Err(AppError::Generic(
            "Hostname must be 253 characters or fewer".into(),
        ));
    }
    if hostname.starts_with('.') || hostname.ends_with('.') || hostname.contains("..") {
        return Err(AppError::Generic("Hostname format is invalid".into()));
    }

    for label in hostname.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(AppError::Generic("Hostname format is invalid".into()));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(AppError::Generic("Hostname format is invalid".into()));
        }
        if !label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(AppError::Generic(
                "Hostname may only contain letters, numbers, hyphens, and dots".into(),
            ));
        }
    }

    Ok(Some(hostname))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn unique_name(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{prefix}-{}-{nanos}", std::process::id())
    }

    struct TempStateDir {
        previous: Option<std::ffi::OsString>,
        path: PathBuf,
    }

    impl Drop for TempStateDir {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(STATE_DIR_ENV, previous);
            } else {
                std::env::remove_var(STATE_DIR_ENV);
            }
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn with_temp_state_dir(test: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = std::env::temp_dir().join(unique_name("clawmacdo-state"));
        let previous = std::env::var_os(STATE_DIR_ENV);
        std::env::set_var(STATE_DIR_ENV, &path);
        let _temp_state_dir = TempStateDir { previous, path };
        test();
    }

    #[test]
    fn normalize_hostname_accepts_valid_names_and_lowercases() {
        assert_eq!(
            normalize_hostname("  ExAmple-Host.Sub.Domain  ").unwrap(),
            Some("example-host.sub.domain".to_string())
        );
        assert_eq!(normalize_hostname("   ").unwrap(), None);
    }

    #[test]
    fn normalize_hostname_rejects_invalid_names() {
        for invalid in [
            ".example",
            "example.",
            "bad..host",
            "-edge",
            "edge-",
            "bad_host",
        ] {
            assert!(
                normalize_hostname(invalid).is_err(),
                "{invalid} should fail"
            );
        }
    }

    #[test]
    fn explicit_state_dir_accepts_absolute_paths_and_tilde() {
        let absolute = if cfg!(windows) {
            r"C:\clawmacdo-state"
        } else {
            "/app/.clawmacdo"
        };
        assert_eq!(
            explicit_state_dir(Some(absolute), STATE_DIR_ENV).unwrap(),
            Some(PathBuf::from(absolute))
        );
        assert!(explicit_state_dir(Some("~/state"), STATE_DIR_ENV)
            .unwrap()
            .is_some());
        assert_eq!(explicit_state_dir(Some("  "), STATE_DIR_ENV).unwrap(), None);
    }

    #[test]
    fn explicit_state_dir_rejects_relative_paths() {
        let err = explicit_state_dir(Some("relative/.clawmacdo"), STATE_DIR_ENV).unwrap_err();
        assert!(err.to_string().contains("absolute path"));
    }

    #[test]
    fn resolve_key_path_accepts_files_within_keys_dir() {
        with_temp_state_dir(|| {
            let keys_dir = keys_dir().unwrap();
            std::fs::create_dir_all(&keys_dir).unwrap();

            let file_path = keys_dir.join(unique_name("key"));
            std::fs::write(&file_path, "ssh-private-key").unwrap();

            let resolved = resolve_key_path(file_path.to_str().unwrap()).unwrap();
            let expected = std::fs::canonicalize(&file_path).unwrap();
            assert_eq!(resolved, expected);

            let _ = std::fs::remove_file(file_path);
        });
    }

    #[test]
    fn resolve_key_path_rejects_files_outside_keys_dir() {
        with_temp_state_dir(|| {
            let keys_dir = keys_dir().unwrap();
            std::fs::create_dir_all(&keys_dir).unwrap();

            let outside_path = std::env::temp_dir().join(unique_name("outside-key"));
            std::fs::write(&outside_path, "ssh-private-key").unwrap();

            let err = resolve_key_path(outside_path.to_str().unwrap()).unwrap_err();
            assert!(err.to_string().contains("must stay within"));

            let _ = std::fs::remove_file(outside_path);
        });
    }
}
