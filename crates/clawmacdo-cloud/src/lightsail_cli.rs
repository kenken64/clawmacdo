use async_trait::async_trait;
use clawmacdo_core::error::AppError;
use serde::Deserialize;
use std::process::Command;
use tokio::time::{sleep, Duration, Instant};

use crate::cloud_provider::{CloudProvider, CreateInstanceParams, InstanceInfo, KeyInfo};

pub struct LightsailCliProvider {
    region: String,
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

impl LightsailCliProvider {
    pub fn new(region: String) -> Self {
        Self { region }
    }

    /// Execute AWS CLI command and return JSON output
    fn execute_aws_cli(&self, args: &[&str]) -> Result<String, AppError> {
        let output = Command::new("aws")
            .arg("lightsail")
            .args(args)
            .arg("--region")
            .arg(&self.region)
            .arg("--output")
            .arg("json")
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
