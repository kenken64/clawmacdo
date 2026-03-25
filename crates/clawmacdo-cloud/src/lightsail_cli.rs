use async_trait::async_trait;
use clawmacdo_core::error::AppError;
use serde::Deserialize;
use std::process::Command;
use tokio::time::{sleep, Duration, Instant};

use crate::cloud_provider::{CloudProvider, CreateInstanceParams, InstanceInfo, KeyInfo};

/// On Windows, AWS CLI v2 installs to a fixed directory that may not be in the
/// current process PATH.  Probe the known install location and prepend it if found.
#[cfg(not(target_os = "windows"))]
fn patch_aws_path() {}

#[cfg(target_os = "windows")]
fn patch_aws_path() {
    let candidates = [
        r"C:\Program Files\Amazon\AWSCLIV2",
        r"C:\Program Files (x86)\Amazon\AWSCLIV2",
    ];
    for dir in &candidates {
        if std::path::Path::new(dir).exists() {
            let current = std::env::var("PATH").unwrap_or_default();
            if !current
                .to_ascii_lowercase()
                .contains(&dir.to_ascii_lowercase())
            {
                std::env::set_var("PATH", format!("{dir};{current}"));
            }
            break;
        }
    }
}

/// Check that the AWS CLI is installed, and attempt to install it automatically
/// if it is missing.  Returns `Ok(())` when the CLI is available, or an error
/// describing what went wrong.
pub fn ensure_aws_cli() -> Result<(), AppError> {
    // On Windows, aws may already be installed but not in the current process PATH.
    // Patch PATH with the known install directory before the first probe.
    #[cfg(target_os = "windows")]
    patch_aws_path();

    if Command::new("aws")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    eprintln!("AWS CLI not found — attempting auto-install...");

    let installed = if cfg!(target_os = "macos") {
        // `brew upgrade` installs if missing and upgrades if present; always gets latest.
        Command::new("brew")
            .args(["upgrade", "awscli"])
            .status()
            .map(|s| s.success())
            .unwrap_or_else(|_| {
                // brew not found — try install as fallback
                Command::new("brew")
                    .args(["install", "awscli"])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            })
    } else if cfg!(target_os = "linux") {
        Command::new("sh")
            .args([
                "-c",
                "curl -fsSL https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip -o /tmp/awscliv2.zip \
                 && unzip -qo /tmp/awscliv2.zip -d /tmp \
                 && sudo /tmp/aws/install --update \
                 && rm -rf /tmp/awscliv2.zip /tmp/aws",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else if cfg!(target_os = "windows") {
        // Try winget first (built into Windows 10/11).
        // winget returns a non-zero exit code when the package is already at the
        // latest version ("No available upgrade found"), so check the output text
        // as well as the exit code.
        // Use `winget upgrade` so we always get the latest release.
        // Falls back gracefully if upgrade reports nothing to do.
        let winget_ok = Command::new("winget")
            .args([
                "upgrade",
                "-e",
                "--id",
                "Amazon.AWSCLI",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ])
            .output()
            .map(|out| {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let combined = format!("{stdout}{stderr}");
                out.status.success()
                    || combined.contains("already installed")
                    || combined.contains("No available upgrade")
                    || combined.contains("No applicable upgrade")
                    || combined.contains("Successfully installed")
                    || combined.contains("Successfully upgraded")
            })
            .unwrap_or(false);

        if winget_ok {
            // winget confirmed aws is on disk; patch PATH so this process can find it.
            patch_aws_path();
            true
        } else {
            // Fallback: download and run the official MSI via PowerShell.
            let msi_ok = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Invoke-WebRequest -Uri https://awscli.amazonaws.com/AWSCLIV2.msi \
                       -OutFile \"$env:TEMP\\AWSCLIV2.msi\"; \
                     Start-Process msiexec.exe \
                       -ArgumentList '/i',(\"$env:TEMP\\AWSCLIV2.msi\"),'/quiet','/norestart' \
                       -Wait; \
                     Remove-Item \"$env:TEMP\\AWSCLIV2.msi\" -Force",
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if msi_ok {
                patch_aws_path();
            }
            msi_ok
        }
    } else {
        false
    };

    if !installed
        || !Command::new("aws")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    {
        return Err(AppError::CloudProviderError(
            "AWS CLI not found and auto-install failed. Please install manually: https://aws.amazon.com/cli/".into(),
        ));
    }

    eprintln!("AWS CLI installed successfully.");
    Ok(())
}

pub struct LightsailCliProvider {
    region: String,
    access_key: Option<String>,
    secret_key: Option<String>,
}

#[derive(Deserialize)]
struct LightsailOperationResponse {
    operation: Option<LightsailOperation>,
}

#[derive(Deserialize)]
struct LightsailOperation {
    #[serde(rename = "resourceName")]
    resource_name: Option<String>,
    #[allow(dead_code)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct LightsailInstanceResponse {
    instance: Option<LightsailInstance>,
}

#[derive(Deserialize)]
struct LightsailInstance {
    name: Option<String>,
    state: Option<LightsailInstanceState>,
    #[serde(rename = "publicIpAddress")]
    public_ip_address: Option<String>,
}

#[derive(Deserialize)]
struct LightsailInstanceState {
    name: Option<String>,
}

#[derive(Deserialize)]
struct LightsailInstancesResponse {
    instances: Option<Vec<LightsailInstance>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LightsailSnapshot {
    pub name: Option<String>,
    pub state: Option<String>,
    #[serde(rename = "fromInstanceName")]
    pub from_instance_name: Option<String>,
    #[serde(rename = "fromBundleId")]
    pub from_bundle_id: Option<String>,
    #[serde(rename = "sizeInGb")]
    pub size_in_gb: Option<u64>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<f64>,
}

#[derive(Deserialize)]
struct LightsailSnapshotResponse {
    #[serde(rename = "instanceSnapshot")]
    instance_snapshot: Option<LightsailSnapshot>,
}

#[derive(Deserialize)]
struct LightsailSnapshotsResponse {
    #[serde(rename = "instanceSnapshots")]
    instance_snapshots: Option<Vec<LightsailSnapshot>>,
}

impl LightsailCliProvider {
    pub fn new(region: String) -> Self {
        Self {
            region,
            access_key: None,
            secret_key: None,
        }
    }

    pub fn with_credentials(region: String, access_key: String, secret_key: String) -> Self {
        Self {
            region,
            access_key: if access_key.is_empty() {
                None
            } else {
                Some(access_key)
            },
            secret_key: if secret_key.is_empty() {
                None
            } else {
                Some(secret_key)
            },
        }
    }

    /// Execute AWS CLI command and return JSON output
    fn execute_aws_cli(&self, args: &[&str]) -> Result<String, AppError> {
        let mut cmd = Command::new("aws");
        cmd.arg("lightsail")
            .args(args)
            .arg("--region")
            .arg(&self.region)
            .arg("--output")
            .arg("json");
        if let Some(ak) = &self.access_key {
            cmd.env("AWS_ACCESS_KEY_ID", ak);
        }
        if let Some(sk) = &self.secret_key {
            cmd.env("AWS_SECRET_ACCESS_KEY", sk);
        }
        // Prevent stale credentials in ~/.aws/credentials from overriding
        // the explicitly provided keys.
        if self.access_key.is_some() || self.secret_key.is_some() {
            cmd.env("AWS_SHARED_CREDENTIALS_FILE", "/dev/null");
            cmd.env("AWS_CONFIG_FILE", "/dev/null");
        }
        let output = cmd
            .output()
            .map_err(|e| AppError::CloudProviderError(format!("Failed to execute AWS CLI: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::CloudProviderError(format!(
                "AWS CLI command failed: {stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.to_string())
    }

    /// Map common sizes to Lightsail bundle IDs
    fn map_size_to_bundle(&self, size: &str) -> &str {
        match size {
            "s-1vcpu-2gb" | "1vcpu-2gb" => "small_3_0", // $10/month - 1vCPU, 2GB RAM
            "s-2vcpu-4gb" | "2vcpu-4gb" => "medium_3_0", // $20/month - 2vCPU, 4GB RAM ⭐
            "s-4vcpu-8gb" | "4vcpu-8gb" => "large_3_0", // $40/month - 4vCPU, 8GB RAM
            _ => "medium_3_0", // Default to 2vCPU, 4GB for the user's request
        }
    }

    // --- Snapshot methods ---

    /// Create an instance snapshot. Returns immediately; poll with `get_snapshot`.
    pub fn create_instance_snapshot(
        &self,
        instance_name: &str,
        snapshot_name: &str,
    ) -> Result<(), AppError> {
        self.execute_aws_cli(&[
            "create-instance-snapshot",
            "--instance-snapshot-name",
            snapshot_name,
            "--instance-name",
            instance_name,
        ])?;
        Ok(())
    }

    /// Get a single snapshot by name.
    pub fn get_snapshot(&self, snapshot_name: &str) -> Result<LightsailSnapshot, AppError> {
        let output = self.execute_aws_cli(&[
            "get-instance-snapshot",
            "--instance-snapshot-name",
            snapshot_name,
        ])?;
        let resp: LightsailSnapshotResponse = serde_json::from_str(&output).map_err(|e| {
            AppError::CloudProviderError(format!("Failed to parse snapshot response: {e}"))
        })?;
        resp.instance_snapshot.ok_or_else(|| {
            AppError::CloudProviderError(format!("Snapshot '{snapshot_name}' not found"))
        })
    }

    /// List all instance snapshots.
    pub fn list_snapshots(&self) -> Result<Vec<LightsailSnapshot>, AppError> {
        let output = self.execute_aws_cli(&["get-instance-snapshots"])?;
        let resp: LightsailSnapshotsResponse = serde_json::from_str(&output).map_err(|e| {
            AppError::CloudProviderError(format!("Failed to parse snapshots response: {e}"))
        })?;
        Ok(resp.instance_snapshots.unwrap_or_default())
    }

    /// Poll until a snapshot becomes available.
    pub async fn wait_for_snapshot(
        &self,
        snapshot_name: &str,
        timeout: Duration,
    ) -> Result<(), AppError> {
        let start = Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err(AppError::Timeout(
                    "Lightsail snapshot to become available".into(),
                ));
            }
            sleep(Duration::from_secs(10)).await;

            match self.get_snapshot(snapshot_name) {
                Ok(snap) => {
                    let state = snap.state.as_deref().unwrap_or("");
                    if state == "available" {
                        return Ok(());
                    }
                    if state == "error" {
                        return Err(AppError::CloudProviderError(format!(
                            "Snapshot '{snapshot_name}' failed"
                        )));
                    }
                }
                Err(_) => continue,
            }
        }
    }

    /// Create an instance from a snapshot.
    pub fn create_instance_from_snapshot(
        &self,
        instance_name: &str,
        snapshot_name: &str,
        bundle_id: &str,
        key_pair_name: &str,
    ) -> Result<(), AppError> {
        let az = format!("{}a", self.region);
        let tags = r#"[{"key":"openclaw","value":"true"}]"#;

        let mut args = vec![
            "create-instances-from-snapshot",
            "--instance-snapshot-name",
            snapshot_name,
            "--instance-names",
            instance_name,
            "--availability-zone",
            &az,
            "--bundle-id",
            bundle_id,
            "--tags",
            tags,
        ];

        if !key_pair_name.is_empty() {
            args.push("--key-pair-name");
            args.push(key_pair_name);
        }

        self.execute_aws_cli(&args)?;
        Ok(())
    }

    /// Public accessor for bundle mapping.
    pub fn get_bundle_id(&self, size: &str) -> String {
        self.map_size_to_bundle(size).to_string()
    }
}

#[async_trait]
impl CloudProvider for LightsailCliProvider {
    async fn upload_ssh_key(&self, name: &str, public_key: &str) -> Result<KeyInfo, AppError> {
        let output = self.execute_aws_cli(&[
            "import-key-pair",
            "--key-pair-name",
            name,
            "--public-key-base64",
            public_key,
        ])?;

        let response: LightsailOperationResponse = serde_json::from_str(&output).map_err(|e| {
            AppError::CloudProviderError(format!("Failed to parse AWS response: {e}"))
        })?;

        let fingerprint = response
            .operation
            .and_then(|op| op.resource_name)
            .unwrap_or_default();

        Ok(KeyInfo {
            id: name.to_string(),
            fingerprint: Some(fingerprint),
        })
    }

    async fn delete_ssh_key(&self, key_id: &str) -> Result<(), AppError> {
        self.execute_aws_cli(&["delete-key-pair", "--key-pair-name", key_id])?;
        Ok(())
    }

    async fn create_instance(
        &self,
        params: CreateInstanceParams,
    ) -> Result<InstanceInfo, AppError> {
        let bundle_id = self.map_size_to_bundle(&params.size);
        let blueprint_id = if params.image.is_empty() {
            "ubuntu_24_04"
        } else {
            &params.image
        };
        let availability_zone = format!("{}a", self.region); // Convert region to AZ

        let mut args = vec![
            "create-instances",
            "--instance-names",
            &params.name,
            "--blueprint-id",
            blueprint_id,
            "--bundle-id",
            bundle_id,
            "--availability-zone",
            &availability_zone,
        ];

        // Add SSH key if provided
        if !params.ssh_key_id.is_empty() {
            args.push("--key-pair-name");
            args.push(&params.ssh_key_id);
        }

        // Add user data if provided
        if !params.user_data.is_empty() {
            args.push("--user-data");
            args.push(&params.user_data);
        }

        // Add tags
        let tags_json = format!(
            r#"[{{"key":"openclaw","value":"true"}},{{"key":"customer_email","value":"{}"}}"#,
            params.customer_email
        );

        // Add custom tags
        let custom_tags: Vec<String> = params
            .tags
            .iter()
            .filter_map(|tag| {
                if let Some((key, value)) = tag.split_once('=') {
                    Some(format!(r#"{{"key":"{key}","value":"{value}"}}"#))
                } else {
                    None
                }
            })
            .collect();

        let full_tags_json = if custom_tags.is_empty() {
            format!("{tags_json}]")
        } else {
            format!("{},{}]", tags_json, custom_tags.join(","))
        };

        args.push("--tags");
        args.push(&full_tags_json);

        let output = self.execute_aws_cli(&args)?;

        let response: LightsailOperationResponse = serde_json::from_str(&output).map_err(|e| {
            AppError::CloudProviderError(format!("Failed to parse AWS response: {e}"))
        })?;

        let instance_name = response
            .operation
            .and_then(|op| op.resource_name)
            .unwrap_or_else(|| params.name.clone());

        Ok(InstanceInfo {
            id: instance_name.clone(),
            name: instance_name,
            status: "pending".to_string(),
            public_ip: None,
        })
    }

    async fn wait_for_active(
        &self,
        instance_id: &str,
        timeout_secs: u64,
    ) -> Result<InstanceInfo, AppError> {
        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        loop {
            if start.elapsed() > timeout {
                return Err(AppError::CloudProviderError(format!(
                    "Timeout waiting for instance {instance_id} to become active"
                )));
            }

            let output = self.execute_aws_cli(&["get-instance", "--instance-name", instance_id])?;

            let response: LightsailInstanceResponse =
                serde_json::from_str(&output).map_err(|e| {
                    AppError::CloudProviderError(format!("Failed to parse AWS response: {e}"))
                })?;

            if let Some(instance) = response.instance {
                let status = instance
                    .state
                    .and_then(|state| state.name)
                    .unwrap_or_default();

                let public_ip = instance.public_ip_address;

                if status == "running" && public_ip.is_some() {
                    return Ok(InstanceInfo {
                        id: instance_id.to_string(),
                        name: instance.name.unwrap_or_default(),
                        status: "active".to_string(),
                        public_ip,
                    });
                }
            }

            sleep(Duration::from_secs(10)).await;
        }
    }

    async fn delete_instance(&self, instance_id: &str) -> Result<(), AppError> {
        self.execute_aws_cli(&["delete-instance", "--instance-name", instance_id])?;
        Ok(())
    }

    async fn list_instances(&self, _tag: &str) -> Result<Vec<InstanceInfo>, AppError> {
        let output = self.execute_aws_cli(&["get-instances"])?;

        let response: LightsailInstancesResponse = serde_json::from_str(&output).map_err(|e| {
            AppError::CloudProviderError(format!("Failed to parse AWS response: {e}"))
        })?;

        let instances = response
            .instances
            .unwrap_or_default()
            .into_iter()
            .filter(|_| {
                // Note: This is simplified - in real implementation we'd check tags
                // For now, we'll return all instances and let the caller filter
                true
            })
            .map(|instance| {
                let name = instance.name.clone().unwrap_or_default();
                let status = instance
                    .state
                    .and_then(|state| state.name)
                    .unwrap_or_default();

                InstanceInfo {
                    id: name.clone(),
                    name,
                    status,
                    public_ip: instance.public_ip_address,
                }
            })
            .collect();

        Ok(instances)
    }
}
