use crate::cloud_provider::{CloudProvider, CreateInstanceParams, InstanceInfo, KeyInfo};
use crate::error::AppError;
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const CVM_HOST: &str = "cvm.tencentcloudapi.com";
const CVM_ENDPOINT: &str = "https://cvm.tencentcloudapi.com";
const VPC_HOST: &str = "vpc.tencentcloudapi.com";
const VPC_ENDPOINT: &str = "https://vpc.tencentcloudapi.com";

pub const DEFAULT_TENCENT_REGION: &str = "ap-singapore";
pub const DEFAULT_TENCENT_INSTANCE_TYPE: &str = "S5.MEDIUM4";
pub const DEFAULT_TENCENT_IMAGE_ID: &str = "img-487zeit5"; // Ubuntu 24.04 LTS

pub struct TencentClient {
    client: Client,
    secret_id: String,
    secret_key: String,
    region: String,
}

// --- TC3-HMAC-SHA256 Signing ---

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

impl TencentClient {
    pub fn new(secret_id: &str, secret_key: &str, region: &str) -> Result<Self, AppError> {
        let client = Client::builder().build()?;
        Ok(Self {
            client,
            secret_id: secret_id.to_string(),
            secret_key: secret_key.to_string(),
            region: region.to_string(),
        })
    }

    /// Build TC3-HMAC-SHA256 authorization header and send POST request.
    async fn signed_request(
        &self,
        service: &str,
        host: &str,
        endpoint: &str,
        action: &str,
        version: &str,
        payload: &str,
    ) -> Result<serde_json::Value, AppError> {
        let now = Utc::now();
        let timestamp = now.timestamp();
        let date = now.format("%Y-%m-%d").to_string();

        // Step 1: Canonical request
        let hashed_payload = sha256_hex(payload.as_bytes());
        let canonical_request = format!(
            "POST\n/\n\ncontent-type:application/json\nhost:{host}\n\ncontent-type;host\n{hashed_payload}"
        );

        // Step 2: String to sign
        let credential_scope = format!("{date}/{service}/tc3_request");
        let hashed_canonical = sha256_hex(canonical_request.as_bytes());
        let string_to_sign = format!(
            "TC3-HMAC-SHA256\n{timestamp}\n{credential_scope}\n{hashed_canonical}"
        );

        // Step 3: Signing key
        let secret_date = hmac_sha256(
            format!("TC3{}", self.secret_key).as_bytes(),
            date.as_bytes(),
        );
        let secret_service = hmac_sha256(&secret_date, service.as_bytes());
        let secret_signing = hmac_sha256(&secret_service, b"tc3_request");

        // Step 4: Signature
        let signature = hex::encode(hmac_sha256(&secret_signing, string_to_sign.as_bytes()));

        let authorization = format!(
            "TC3-HMAC-SHA256 Credential={}/{}, SignedHeaders=content-type;host, Signature={}",
            self.secret_id, credential_scope, signature
        );

        let resp = self
            .client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .header("Host", host)
            .header("Authorization", &authorization)
            .header("X-TC-Action", action)
            .header("X-TC-Timestamp", timestamp.to_string())
            .header("X-TC-Version", version)
            .header("X-TC-Region", &self.region)
            .body(payload.to_string())
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::TencentCloud(format!(
                "{action} failed ({status}): {text}"
            )));
        }

        let body: serde_json::Value = resp.json().await?;

        // Check for API-level errors
        if let Some(error) = body.get("Response").and_then(|r| r.get("Error")) {
            let code = error
                .get("Code")
                .and_then(|c| c.as_str())
                .unwrap_or("Unknown");
            let msg = error
                .get("Message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(AppError::TencentCloud(format!(
                "{action} error ({code}): {msg}"
            )));
        }

        Ok(body)
    }

    async fn cvm_request(
        &self,
        action: &str,
        payload: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.signed_request("cvm", CVM_HOST, CVM_ENDPOINT, action, "2017-03-12", payload)
            .await
    }

    async fn vpc_request(
        &self,
        action: &str,
        payload: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.signed_request("vpc", VPC_HOST, VPC_ENDPOINT, action, "2017-03-12", payload)
            .await
    }

    /// Import an SSH key pair into Tencent Cloud.
    pub async fn import_key_pair(
        &self,
        name: &str,
        public_key: &str,
    ) -> Result<KeyInfo, AppError> {
        let payload = serde_json::json!({
            "KeyName": name,
            "PublicKey": public_key
        });
        let resp = self
            .cvm_request("ImportKeyPair", &payload.to_string())
            .await?;

        let key_id = resp["Response"]["KeyId"]
            .as_str()
            .ok_or_else(|| AppError::TencentCloud("Missing KeyId in response".into()))?
            .to_string();

        Ok(KeyInfo {
            id: key_id,
            fingerprint: None,
        })
    }

    /// Delete an SSH key pair.
    pub async fn delete_key_pair(&self, key_id: &str) -> Result<(), AppError> {
        let payload = serde_json::json!({
            "KeyIds": [key_id]
        });
        self.cvm_request("DeleteKeyPairs", &payload.to_string())
            .await?;
        Ok(())
    }

    /// Create a CVM instance.
    pub async fn create_instance(
        &self,
        name: &str,
        instance_type: &str,
        image_id: &str,
        key_id: &str,
        user_data_base64: &str,
    ) -> Result<String, AppError> {
        let payload = serde_json::json!({
            "InstanceName": name,
            "InstanceType": instance_type,
            "ImageId": image_id,
            "SystemDisk": {
                "DiskType": "CLOUD_BSSD",
                "DiskSize": 50
            },
            "InternetAccessible": {
                "InternetChargeType": "TRAFFIC_POSTPAID_BY_HOUR",
                "InternetMaxBandwidthOut": 100,
                "PublicIpAssigned": true
            },
            "LoginSettings": {
                "KeyIds": [key_id]
            },
            "Placement": {
                "Zone": format!("{}-1", self.region)
            },
            "InstanceChargeType": "POSTPAID_BY_HOUR",
            "InstanceCount": 1,
            "UserData": user_data_base64,
            "TagSpecification": [{
                "ResourceType": "instance",
                "Tags": [{
                    "Key": "app",
                    "Value": "openclaw"
                }]
            }],
            "EnhancedService": {
                "SecurityService": { "Enabled": true },
                "MonitorService": { "Enabled": true }
            }
        });

        let resp = self
            .cvm_request("RunInstances", &payload.to_string())
            .await?;

        let instance_id = resp["Response"]["InstanceIdSet"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::TencentCloud("Missing InstanceId in response".into()))?
            .to_string();

        Ok(instance_id)
    }

    /// Get instance details by ID.
    pub async fn describe_instance(&self, instance_id: &str) -> Result<InstanceInfo, AppError> {
        let payload = serde_json::json!({
            "InstanceIds": [instance_id]
        });

        let resp = self
            .cvm_request("DescribeInstances", &payload.to_string())
            .await?;

        let instance = resp["Response"]["InstanceSet"]
            .as_array()
            .and_then(|arr| arr.first())
            .ok_or_else(|| {
                AppError::TencentCloud(format!("Instance {instance_id} not found"))
            })?;

        let name = instance["InstanceName"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let status = instance["InstanceState"]
            .as_str()
            .unwrap_or("UNKNOWN")
            .to_string();
        let public_ip = instance["PublicIpAddresses"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(InstanceInfo {
            id: instance_id.to_string(),
            name,
            status,
            public_ip,
        })
    }

    /// List all instances tagged with app=openclaw.
    pub async fn list_openclaw_instances(&self) -> Result<Vec<InstanceInfo>, AppError> {
        let payload = serde_json::json!({
            "Filters": [{
                "Name": "tag:app",
                "Values": ["openclaw"]
            }],
            "Limit": 100
        });

        let resp = self
            .cvm_request("DescribeInstances", &payload.to_string())
            .await?;

        let instances = resp["Response"]["InstanceSet"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        Ok(instances
            .iter()
            .map(|inst| {
                let id = inst["InstanceId"].as_str().unwrap_or("").to_string();
                let name = inst["InstanceName"].as_str().unwrap_or("").to_string();
                let status = inst["InstanceState"].as_str().unwrap_or("UNKNOWN").to_string();
                let public_ip = inst["PublicIpAddresses"]
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                InstanceInfo {
                    id,
                    name,
                    status,
                    public_ip,
                }
            })
            .collect())
    }

    /// Terminate (destroy) an instance.
    pub async fn terminate_instance(&self, instance_id: &str) -> Result<(), AppError> {
        let payload = serde_json::json!({
            "InstanceIds": [instance_id]
        });
        self.cvm_request("TerminateInstances", &payload.to_string())
            .await?;
        Ok(())
    }

    /// Poll until instance is RUNNING and has a public IP.
    pub async fn wait_for_running(
        &self,
        instance_id: &str,
        timeout: std::time::Duration,
    ) -> Result<InstanceInfo, AppError> {
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err(AppError::Timeout(
                    "Tencent CVM instance to become RUNNING".into(),
                ));
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            match self.describe_instance(instance_id).await {
                Ok(info) => {
                    if info.status == "RUNNING" && info.public_ip.is_some() {
                        return Ok(info);
                    }
                }
                Err(_) => continue,
            }
        }
    }

    /// List SSH key pairs.
    pub async fn list_key_pairs(&self) -> Result<Vec<(String, String)>, AppError> {
        let payload = serde_json::json!({
            "Limit": 100
        });
        let resp = self
            .cvm_request("DescribeKeyPairs", &payload.to_string())
            .await?;

        let keys = resp["Response"]["KeyPairSet"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        Ok(keys
            .iter()
            .map(|k| {
                let id = k["KeyId"].as_str().unwrap_or("").to_string();
                let name = k["KeyName"].as_str().unwrap_or("").to_string();
                (id, name)
            })
            .collect())
    }
}

#[async_trait::async_trait]
impl CloudProvider for TencentClient {
    async fn upload_ssh_key(&self, name: &str, public_key: &str) -> Result<KeyInfo, AppError> {
        self.import_key_pair(name, public_key).await
    }

    async fn delete_ssh_key(&self, key_id: &str) -> Result<(), AppError> {
        self.delete_key_pair(key_id).await
    }

    async fn create_instance(
        &self,
        params: CreateInstanceParams,
    ) -> Result<InstanceInfo, AppError> {
        let user_data_b64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &params.user_data);

        let instance_id = TencentClient::create_instance(
            self,
            &params.name,
            &params.size,
            &params.image,
            &params.ssh_key_id,
            &user_data_b64,
        )
        .await?;

        Ok(InstanceInfo {
            id: instance_id,
            name: params.name,
            status: "PENDING".to_string(),
            public_ip: None,
        })
    }

    async fn wait_for_active(
        &self,
        instance_id: &str,
        timeout_secs: u64,
    ) -> Result<InstanceInfo, AppError> {
        self.wait_for_running(instance_id, std::time::Duration::from_secs(timeout_secs))
            .await
    }

    async fn delete_instance(&self, instance_id: &str) -> Result<(), AppError> {
        self.terminate_instance(instance_id).await
    }

    async fn list_instances(&self, _tag: &str) -> Result<Vec<InstanceInfo>, AppError> {
        self.list_openclaw_instances().await
    }
}
