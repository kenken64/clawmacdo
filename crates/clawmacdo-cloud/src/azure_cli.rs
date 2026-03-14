use async_trait::async_trait;
use clawmacdo_core::error::AppError;
use serde::Deserialize;
use std::process::Command;
use tokio::time::{sleep, Duration, Instant};

use crate::cloud_provider::{CloudProvider, CreateInstanceParams, InstanceInfo, KeyInfo};

/// Check that the Azure CLI is installed, and attempt to install it automatically
/// if it is missing.  Returns `Ok(())` when the CLI is available, or an error
/// describing what went wrong.
pub fn ensure_az_cli() -> Result<(), AppError> {
    if Command::new("az")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    eprintln!("Azure CLI not found — attempting auto-install...");

    let installed = if cfg!(target_os = "macos") {
        Command::new("brew")
            .args(["install", "azure-cli"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else if cfg!(target_os = "linux") {
        Command::new("sh")
            .args([
                "-c",
                "curl -sL https://aka.ms/InstallAzureCLIDeb | sudo bash",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        false
    };

    if !installed
        || !Command::new("az")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    {
        return Err(AppError::Azure(
            "Azure CLI not found and auto-install failed. Please install manually: https://learn.microsoft.com/en-us/cli/azure/install-azure-cli".into(),
        ));
    }

    eprintln!("Azure CLI installed successfully.");
    Ok(())
}

/// Login to Azure using a service principal.
pub fn az_login(tenant_id: &str, client_id: &str, client_secret: &str) -> Result<(), AppError> {
    let output = Command::new("az")
        .args([
            "login",
            "--service-principal",
            "-u",
            client_id,
            "-p",
            client_secret,
            "--tenant",
            tenant_id,
            "--output",
            "none",
        ])
        .output()
        .map_err(|e| AppError::Azure(format!("Failed to execute az login: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Azure(format!("az login failed: {stderr}")));
    }

    Ok(())
}

/// Set the active Azure subscription.
pub fn az_set_subscription(subscription_id: &str) -> Result<(), AppError> {
    let output = Command::new("az")
        .args([
            "account",
            "set",
            "--subscription",
            subscription_id,
            "--output",
            "none",
        ])
        .output()
        .map_err(|e| AppError::Azure(format!("Failed to set subscription: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Azure(format!("az account set failed: {stderr}")));
    }

    Ok(())
}

pub struct AzureCliProvider {
    region: String,
    resource_group: String,
    #[allow(dead_code)]
    subscription_id: String,
}

#[derive(Deserialize)]
struct AzureVmCreateResponse {
    id: Option<String>,
    #[serde(rename = "publicIpAddress")]
    public_ip_address: Option<String>,
    #[serde(rename = "powerState")]
    power_state: Option<String>,
}

#[derive(Deserialize)]
struct AzureVmShowResponse {
    name: Option<String>,
    #[serde(rename = "publicIps")]
    public_ips: Option<String>,
    #[serde(rename = "powerState")]
    power_state: Option<String>,
}

#[derive(Deserialize)]
struct AzureVmListEntry {
    name: Option<String>,
    #[serde(rename = "publicIps")]
    public_ips: Option<String>,
    #[serde(rename = "powerState")]
    power_state: Option<String>,
}

impl AzureCliProvider {
    pub fn new(region: String, resource_group: String, subscription_id: String) -> Self {
        Self {
            region,
            resource_group,
            subscription_id,
        }
    }

    /// Execute an az CLI command and return its stdout.
    fn execute_az_cli(&self, args: &[&str]) -> Result<String, AppError> {
        let output = Command::new("az")
            .args(args)
            .arg("--output")
            .arg("json")
            .output()
            .map_err(|e| AppError::Azure(format!("Failed to execute Azure CLI: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Azure(format!(
                "Azure CLI command failed: {stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.to_string())
    }

    /// Ensure the resource group exists.
    pub fn ensure_resource_group(&self) -> Result<(), AppError> {
        self.execute_az_cli(&[
            "group",
            "create",
            "--name",
            &self.resource_group,
            "--location",
            &self.region,
        ])?;
        Ok(())
    }

    /// Delete the entire resource group (cleans up all resources).
    pub fn delete_resource_group(&self) -> Result<(), AppError> {
        self.execute_az_cli(&[
            "group",
            "delete",
            "--name",
            &self.resource_group,
            "--yes",
            "--no-wait",
        ])?;
        Ok(())
    }

    /// Map common size strings to Azure VM SKUs.
    fn map_size_to_azure_sku(size: &str) -> &'static str {
        match size {
            "Standard_B1ms" => "Standard_B1ms",
            "Standard_B2s" => "Standard_B2s",
            "Standard_B2ms" => "Standard_B2ms",
            "Standard_B4ms" => "Standard_B4ms",
            "Standard_D2s_v5" => "Standard_D2s_v5",
            "Standard_D4s_v5" => "Standard_D4s_v5",
            // Map generic sizes to Azure equivalents
            "s-1vcpu-2gb" | "1vcpu-2gb" => "Standard_B1ms",
            "s-2vcpu-4gb" | "2vcpu-4gb" => "Standard_B2s",
            "s-4vcpu-8gb" | "4vcpu-8gb" => "Standard_B4ms",
            _ => "Standard_B2s", // Default
        }
    }
}

#[async_trait]
impl CloudProvider for AzureCliProvider {
    async fn upload_ssh_key(&self, name: &str, _public_key: &str) -> Result<KeyInfo, AppError> {
        // Azure VM creation accepts SSH keys inline — no separate upload step.
        Ok(KeyInfo {
            id: name.to_string(),
            fingerprint: None,
        })
    }

    async fn delete_ssh_key(&self, _key_id: &str) -> Result<(), AppError> {
        // No separate SSH key resource to delete on Azure.
        Ok(())
    }

    async fn create_instance(
        &self,
        params: CreateInstanceParams,
    ) -> Result<InstanceInfo, AppError> {
        let sku = Self::map_size_to_azure_sku(&params.size);
        let image = if params.image.is_empty() {
            clawmacdo_core::config::DEFAULT_AZURE_IMAGE
        } else {
            &params.image
        };

        // Write cloud-init data to a temp file (az vm create --custom-data requires a file path)
        let ci_path = format!("/tmp/clawmacdo-ci-{}.yaml", uuid::Uuid::new_v4());
        std::fs::write(&ci_path, &params.user_data)
            .map_err(|e| AppError::Azure(format!("Failed to write cloud-init temp file: {e}")))?;

        let custom_data_arg = format!("@{ci_path}");
        let tags = format!("openclaw=true customer_email={}", params.customer_email);

        let mut args = vec![
            "vm",
            "create",
            "--resource-group",
            &self.resource_group,
            "--name",
            &params.name,
            "--image",
            image,
            "--size",
            sku,
            "--admin-username",
            "azureuser",
            "--ssh-key-values",
            &params.ssh_key_id, // We pass the actual public key content here
            "--custom-data",
            &custom_data_arg,
            "--tags",
            &tags,
            "--public-ip-sku",
            "Standard",
        ];

        // Add location
        args.push("--location");
        args.push(&self.region);

        let output = self.execute_az_cli(&args);

        // Clean up temp file regardless of result
        let _ = std::fs::remove_file(&ci_path);

        let output = output?;

        let response: AzureVmCreateResponse = serde_json::from_str(&output).map_err(|e| {
            AppError::Azure(format!("Failed to parse Azure VM create response: {e}"))
        })?;

        // Open required ports
        let _ = self.execute_az_cli(&[
            "vm",
            "open-port",
            "--resource-group",
            &self.resource_group,
            "--name",
            &params.name,
            "--port",
            "22,80,443,18789",
            "--priority",
            "100",
        ]);

        Ok(InstanceInfo {
            id: response.id.unwrap_or_else(|| params.name.clone()),
            name: params.name,
            status: response
                .power_state
                .unwrap_or_else(|| "creating".to_string()),
            public_ip: response.public_ip_address,
        })
    }

    async fn wait_for_active(
        &self,
        instance_id: &str,
        timeout_secs: u64,
    ) -> Result<InstanceInfo, AppError> {
        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        // The instance_id for Azure is the VM name (not the full resource ID)
        let vm_name = instance_id.rsplit('/').next().unwrap_or(instance_id);

        loop {
            if start.elapsed() > timeout {
                return Err(AppError::Azure(format!(
                    "Timeout waiting for VM {vm_name} to become active"
                )));
            }

            let output = self.execute_az_cli(&[
                "vm",
                "show",
                "--resource-group",
                &self.resource_group,
                "--name",
                vm_name,
                "--show-details",
            ])?;

            let response: AzureVmShowResponse = serde_json::from_str(&output).map_err(|e| {
                AppError::Azure(format!("Failed to parse Azure VM show response: {e}"))
            })?;

            let power_state = response.power_state.unwrap_or_default();
            let public_ip = response.public_ips.as_ref().and_then(|ip| {
                let trimmed = ip.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });

            if power_state.contains("running") && public_ip.is_some() {
                return Ok(InstanceInfo {
                    id: vm_name.to_string(),
                    name: response.name.unwrap_or_else(|| vm_name.to_string()),
                    status: "active".to_string(),
                    public_ip,
                });
            }

            sleep(Duration::from_secs(10)).await;
        }
    }

    async fn delete_instance(&self, instance_id: &str) -> Result<(), AppError> {
        let vm_name = instance_id.rsplit('/').next().unwrap_or(instance_id);

        self.execute_az_cli(&[
            "vm",
            "delete",
            "--resource-group",
            &self.resource_group,
            "--name",
            vm_name,
            "--yes",
            "--force-deletion",
            "true",
        ])?;
        Ok(())
    }

    async fn list_instances(&self, _tag: &str) -> Result<Vec<InstanceInfo>, AppError> {
        let output = self.execute_az_cli(&[
            "vm",
            "list",
            "--resource-group",
            &self.resource_group,
            "--show-details",
        ])?;

        let vms: Vec<AzureVmListEntry> = serde_json::from_str(&output)
            .map_err(|e| AppError::Azure(format!("Failed to parse Azure VM list response: {e}")))?;

        let instances = vms
            .into_iter()
            .map(|vm| {
                let name = vm.name.clone().unwrap_or_default();
                let public_ip = vm.public_ips.and_then(|ip| {
                    let trimmed = ip.trim().to_string();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                });
                InstanceInfo {
                    id: name.clone(),
                    name,
                    status: vm.power_state.unwrap_or_default(),
                    public_ip,
                }
            })
            .collect();

        Ok(instances)
    }
}
