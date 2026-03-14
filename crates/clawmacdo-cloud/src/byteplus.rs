use crate::cloud_provider::{CloudProvider, CreateInstanceParams, InstanceInfo, KeyInfo};
use chrono::Utc;
use clawmacdo_core::error::AppError;
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const ECS_SERVICE: &str = "ecs";
const ECS_VERSION: &str = "2020-04-01";
const VPC_SERVICE: &str = "vpc";
const VPC_VERSION: &str = "2020-04-01";
const DEFAULT_ZONE_SUFFIX: &str = "a";
const DEFAULT_IMAGE_ID: &str = "image_ubuntu_2404_64_20G";

pub struct BytePlusClient {
    client: Client,
    access_key: String,
    secret_key: String,
    region: String,
}

// --- HMAC-SHA256 Signing helpers ---

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

impl BytePlusClient {
    pub fn new(access_key: &str, secret_key: &str, region: &str) -> Result<Self, AppError> {
        let client = Client::builder().build()?;
        Ok(Self {
            client,
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            region: region.to_string(),
        })
    }

    fn host_for_service(&self, service: &str) -> String {
        format!("open.{service}.byteplusapi.com")
    }

    fn endpoint_for_service(&self, service: &str) -> String {
        format!("https://open.{service}.byteplusapi.com")
    }

    /// Build HMAC-SHA256 authorization header and send POST request.
    ///
    /// BytePlus signing is similar to AWS SigV4:
    /// - Auth prefix: `HMAC-SHA256`
    /// - Key derivation seed: bare `secret_key` (not prefixed)
    /// - Timestamp header: `X-Date: YYYYMMDDThhmmssZ`
    /// - API version/action via query params: `?Action={action}&Version={version}`
    /// - Credential scope: `{date}/{region}/{service}/request`
    /// - Signed headers: `content-type;host;x-date`
    async fn signed_request(
        &self,
        service: &str,
        action: &str,
        version: &str,
        payload: &str,
    ) -> Result<serde_json::Value, AppError> {
        let host = self.host_for_service(service);
        let endpoint = self.endpoint_for_service(service);
        let now = Utc::now();
        let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();

        let query_string = format!("Action={action}&Version={version}");

        // Step 1: Canonical request
        let hashed_payload = sha256_hex(payload.as_bytes());
        let canonical_request = format!(
            "POST\n/\n{query_string}\ncontent-type:application/json\nhost:{host}\nx-date:{x_date}\n\ncontent-type;host;x-date\n{hashed_payload}"
        );

        // Step 2: String to sign
        let credential_scope = format!("{date}/{}/{service}/request", self.region);
        let hashed_canonical = sha256_hex(canonical_request.as_bytes());
        let string_to_sign =
            format!("HMAC-SHA256\n{x_date}\n{credential_scope}\n{hashed_canonical}");

        // Step 3: Signing key derivation (bare secret_key, no prefix)
        let k_date = hmac_sha256(self.secret_key.as_bytes(), date.as_bytes());
        let k_region = hmac_sha256(&k_date, self.region.as_bytes());
        let k_service = hmac_sha256(&k_region, service.as_bytes());
        let k_signing = hmac_sha256(&k_service, b"request");

        // Step 4: Signature
        let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        let authorization = format!(
            "HMAC-SHA256 Credential={}/{}, SignedHeaders=content-type;host;x-date, Signature={}",
            self.access_key, credential_scope, signature
        );

        let url = format!("{endpoint}/?{query_string}");
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Host", &host)
            .header("X-Date", &x_date)
            .header("Authorization", &authorization)
            .body(payload.to_string())
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::BytePlus(format!(
                "{action} failed ({status}): {text}"
            )));
        }

        let body: serde_json::Value = resp.json().await?;

        // Check for API-level errors
        if let Some(error) = body.get("ResponseMetadata").and_then(|r| r.get("Error")) {
            let code = error
                .get("Code")
                .and_then(|c| c.as_str())
                .unwrap_or("Unknown");
            if !code.is_empty() {
                let msg = error
                    .get("Message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error");
                return Err(AppError::BytePlus(format!(
                    "{action} error ({code}): {msg}"
                )));
            }
        }

        Ok(body)
    }

    async fn ecs_request(
        &self,
        action: &str,
        payload: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.signed_request(ECS_SERVICE, action, ECS_VERSION, payload)
            .await
    }

    async fn vpc_request(
        &self,
        action: &str,
        payload: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.signed_request(VPC_SERVICE, action, VPC_VERSION, payload)
            .await
    }

    /// Import an SSH key pair into BytePlus ECS.
    pub async fn import_key_pair(&self, name: &str, public_key: &str) -> Result<KeyInfo, AppError> {
        let payload = serde_json::json!({
            "KeyPairName": name,
            "PublicKey": public_key
        });
        let resp = self
            .ecs_request("ImportKeyPair", &payload.to_string())
            .await?;

        let key_id = resp["Result"]["KeyPairId"]
            .as_str()
            .or_else(|| resp["Result"]["KeyPairName"].as_str())
            .ok_or_else(|| AppError::BytePlus("Missing KeyPairId in response".into()))?
            .to_string();

        Ok(KeyInfo {
            id: key_id,
            fingerprint: resp["Result"]["KeyPairFingerPrint"]
                .as_str()
                .map(|s| s.to_string()),
        })
    }

    /// Delete an SSH key pair.
    pub async fn delete_key_pair(&self, key_pair_name: &str) -> Result<(), AppError> {
        let payload = serde_json::json!({
            "KeyPairNames": [key_pair_name]
        });
        self.ecs_request("DeleteKeyPairs", &payload.to_string())
            .await?;
        Ok(())
    }

    /// Find or create a VPC tagged `openclaw`.
    async fn ensure_vpc(&self) -> Result<String, AppError> {
        // List existing VPCs and look for one tagged openclaw
        let list_payload = serde_json::json!({
            "MaxResults": 100,
            "TagFilters": [{
                "Key": "app",
                "Values": ["openclaw"]
            }]
        });
        let resp = self
            .vpc_request("DescribeVpcs", &list_payload.to_string())
            .await?;

        if let Some(vpcs) = resp["Result"]["Vpcs"].as_array() {
            for vpc in vpcs {
                if let Some(vpc_id) = vpc["VpcId"].as_str() {
                    if vpc["Status"].as_str() == Some("Available") {
                        return Ok(vpc_id.to_string());
                    }
                }
            }
        }

        // Create new VPC
        let create_payload = serde_json::json!({
            "VpcName": "openclaw-vpc",
            "CidrBlock": "172.16.0.0/16",
            "Tags": [{
                "Key": "app",
                "Value": "openclaw"
            }]
        });
        let resp = self
            .vpc_request("CreateVpc", &create_payload.to_string())
            .await?;

        let vpc_id = resp["Result"]["VpcId"]
            .as_str()
            .ok_or_else(|| AppError::BytePlus("Missing VpcId in CreateVpc response".into()))?
            .to_string();

        // Wait briefly for VPC to become available
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        Ok(vpc_id)
    }

    /// Find or create a subnet in the given VPC.
    async fn ensure_subnet(&self, vpc_id: &str) -> Result<String, AppError> {
        let zone_id = format!("{}{DEFAULT_ZONE_SUFFIX}", self.region);

        let list_payload = serde_json::json!({
            "VpcId": vpc_id,
            "MaxResults": 100
        });
        let resp = self
            .vpc_request("DescribeSubnets", &list_payload.to_string())
            .await?;

        if let Some(subnets) = resp["Result"]["Subnets"].as_array() {
            for subnet in subnets {
                if subnet["ZoneId"].as_str() == Some(&zone_id)
                    && subnet["Status"].as_str() == Some("Available")
                {
                    if let Some(subnet_id) = subnet["SubnetId"].as_str() {
                        return Ok(subnet_id.to_string());
                    }
                }
            }
        }

        // Create new subnet
        let create_payload = serde_json::json!({
            "VpcId": vpc_id,
            "SubnetName": "openclaw-subnet",
            "CidrBlock": "172.16.0.0/24",
            "ZoneId": zone_id,
            "Tags": [{
                "Key": "app",
                "Value": "openclaw"
            }]
        });
        let resp = self
            .vpc_request("CreateSubnet", &create_payload.to_string())
            .await?;

        let subnet_id = resp["Result"]["SubnetId"]
            .as_str()
            .ok_or_else(|| AppError::BytePlus("Missing SubnetId in CreateSubnet response".into()))?
            .to_string();

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        Ok(subnet_id)
    }

    /// Find or create a security group with ports 22, 80, 443, 18789.
    async fn ensure_security_group(&self, vpc_id: &str) -> Result<String, AppError> {
        let list_payload = serde_json::json!({
            "VpcId": vpc_id,
            "MaxResults": 100,
            "TagFilters": [{
                "Key": "app",
                "Values": ["openclaw"]
            }]
        });
        let resp = self
            .vpc_request("DescribeSecurityGroups", &list_payload.to_string())
            .await?;

        if let Some(sgs) = resp["Result"]["SecurityGroups"].as_array() {
            for sg in sgs {
                if let Some(sg_id) = sg["SecurityGroupId"].as_str() {
                    return Ok(sg_id.to_string());
                }
            }
        }

        // Create security group
        let create_payload = serde_json::json!({
            "VpcId": vpc_id,
            "SecurityGroupName": "openclaw-sg",
            "Description": "OpenClaw security group - SSH + HTTP/HTTPS + Gateway",
            "Tags": [{
                "Key": "app",
                "Value": "openclaw"
            }]
        });
        let resp = self
            .vpc_request("CreateSecurityGroup", &create_payload.to_string())
            .await?;

        let sg_id = resp["Result"]["SecurityGroupId"]
            .as_str()
            .ok_or_else(|| {
                AppError::BytePlus("Missing SecurityGroupId in CreateSecurityGroup response".into())
            })?
            .to_string();

        // Add ingress rules
        for (port, desc) in [
            ("22/22", "SSH"),
            ("80/80", "HTTP"),
            ("443/443", "HTTPS"),
            ("18789/18789", "OpenClaw Gateway"),
        ] {
            let rule_payload = serde_json::json!({
                "SecurityGroupId": sg_id,
                "Direction": "ingress",
                "Protocol": "tcp",
                "PortStart": port.split('/').next().unwrap().parse::<i32>().unwrap(),
                "PortEnd": port.split('/').next_back().unwrap().parse::<i32>().unwrap(),
                "CidrIp": "0.0.0.0/0",
                "Description": desc
            });
            self.vpc_request("AuthorizeSecurityGroupIngress", &rule_payload.to_string())
                .await?;
        }

        Ok(sg_id)
    }

    /// Create an ECS instance.
    pub async fn create_instance(
        &self,
        name: &str,
        instance_type: &str,
        key_pair_name: &str,
        user_data_base64: &str,
        customer_email: &str,
    ) -> Result<String, AppError> {
        // Ensure networking resources exist
        let vpc_id = self.ensure_vpc().await?;
        let subnet_id = self.ensure_subnet(&vpc_id).await?;
        let sg_id = self.ensure_security_group(&vpc_id).await?;

        let zone_id = format!("{}{DEFAULT_ZONE_SUFFIX}", self.region);

        let payload = serde_json::json!({
            "InstanceName": name,
            "InstanceTypeId": instance_type,
            "ImageId": DEFAULT_IMAGE_ID,
            "ZoneId": zone_id,
            "SystemVolume": {
                "VolumeType": "ESSD_PL0",
                "Size": 40
            },
            "NetworkInterfaces": [{
                "SubnetId": subnet_id,
                "SecurityGroupIds": [sg_id]
            }],
            "KeyPairName": key_pair_name,
            "InstanceChargeType": "PostPaid",
            "UserData": user_data_base64,
            "Count": 1,
            "Tags": [
                { "Key": "app", "Value": "openclaw" },
                { "Key": "customer_email", "Value": customer_email }
            ]
        });

        let resp = self
            .ecs_request("RunInstances", &payload.to_string())
            .await?;

        let instance_id = resp["Result"]["InstanceIds"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::BytePlus("Missing InstanceId in RunInstances response".into())
            })?
            .to_string();

        Ok(instance_id)
    }

    /// Get instance details by ID.
    pub async fn describe_instance(&self, instance_id: &str) -> Result<InstanceInfo, AppError> {
        let payload = serde_json::json!({
            "InstanceIds": [instance_id]
        });

        let resp = self
            .ecs_request("DescribeInstances", &payload.to_string())
            .await?;

        let instance = resp["Result"]["Instances"]
            .as_array()
            .and_then(|arr| arr.first())
            .ok_or_else(|| AppError::BytePlus(format!("Instance {instance_id} not found")))?;

        let name = instance["InstanceName"].as_str().unwrap_or("").to_string();
        let status = instance["Status"].as_str().unwrap_or("UNKNOWN").to_string();

        // BytePlus ECS: public IP is in EipAddress or NetworkInterfaces
        let public_ip = instance["EipAddress"]["IpAddress"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                instance["NetworkInterfaces"]
                    .as_array()
                    .and_then(|nics| nics.first())
                    .and_then(|nic| nic["PrimaryIpAddress"].as_str())
            })
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
            "TagFilters": [{
                "Key": "app",
                "Values": ["openclaw"]
            }],
            "MaxResults": 100
        });

        let resp = self
            .ecs_request("DescribeInstances", &payload.to_string())
            .await?;

        let instances = resp["Result"]["Instances"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        Ok(instances
            .iter()
            .map(|inst| {
                let id = inst["InstanceId"].as_str().unwrap_or("").to_string();
                let name = inst["InstanceName"].as_str().unwrap_or("").to_string();
                let status = inst["Status"].as_str().unwrap_or("UNKNOWN").to_string();
                let public_ip = inst["EipAddress"]["IpAddress"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        inst["NetworkInterfaces"]
                            .as_array()
                            .and_then(|nics| nics.first())
                            .and_then(|nic| nic["PrimaryIpAddress"].as_str())
                    })
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
        self.ecs_request("DeleteInstance", &payload.to_string())
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
                    "BytePlus ECS instance to become RUNNING".into(),
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
}

#[async_trait::async_trait]
impl CloudProvider for BytePlusClient {
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
        let user_data_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &params.user_data,
        );

        let instance_id = BytePlusClient::create_instance(
            self,
            &params.name,
            &params.size,
            &params.ssh_key_id,
            &user_data_b64,
            &params.customer_email,
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
