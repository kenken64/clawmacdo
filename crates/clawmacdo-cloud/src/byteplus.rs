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
const ARK_SERVICE: &str = "ark";
const ARK_VERSION: &str = "2024-01-01";
const DEFAULT_ZONE_SUFFIX: &str = "a";

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

/// URI-encode a string per SigV4 rules (unreserved chars are not encoded).
fn uri_encode(s: &str) -> String {
    let mut result = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{b:02X}"));
            }
        }
    }
    result
}

/// Flatten a JSON object into dot-notation query parameters (BytePlus GET style).
/// e.g. `{"TagFilters": [{"Key": "app", "Values": ["openclaw"]}]}`
/// becomes `TagFilters.1.Key=app&TagFilters.1.Values.1=openclaw`
fn flatten_to_query_params(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    params: &mut Vec<(String, String)>,
) {
    for (key, value) in obj {
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            serde_json::Value::String(s) => params.push((full_key, s.clone())),
            serde_json::Value::Number(n) => params.push((full_key, n.to_string())),
            serde_json::Value::Bool(b) => params.push((full_key, b.to_string())),
            serde_json::Value::Array(arr) => {
                for (i, item) in arr.iter().enumerate() {
                    let idx_key = format!("{full_key}.{}", i + 1);
                    match item {
                        serde_json::Value::String(s) => params.push((idx_key, s.clone())),
                        serde_json::Value::Number(n) => params.push((idx_key, n.to_string())),
                        serde_json::Value::Bool(b) => params.push((idx_key, b.to_string())),
                        serde_json::Value::Object(inner) => {
                            flatten_to_query_params(inner, &idx_key, params);
                        }
                        _ => {}
                    }
                }
            }
            serde_json::Value::Object(inner) => {
                flatten_to_query_params(inner, &full_key, params);
            }
            _ => {}
        }
    }
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

    /// Resolve endpoint host per the official SDK's DefaultEndpointProvider:
    /// - ARK service: `open.byteplusapi.com` (region-agnostic gateway)
    /// - Bootstrap regions (ap-southeast-2, ap-southeast-3): `{service}.{region}.byteplusapi.com`
    /// - All other regions: `open.ap-southeast-1.byteplusapi.com`
    fn host_for_service(&self, service: &str) -> String {
        if service == ARK_SERVICE {
            return "open.byteplusapi.com".to_string();
        }
        match self.region.as_str() {
            "ap-southeast-2" | "ap-southeast-3" => {
                format!("{service}.{}.byteplusapi.com", self.region)
            }
            _ => "open.ap-southeast-1.byteplusapi.com".to_string(),
        }
    }

    fn endpoint_for_service(&self, service: &str) -> String {
        format!("https://{}", self.host_for_service(service))
    }

    /// Build HMAC-SHA256 authorization header and send a signed request.
    ///
    /// BytePlus signing is similar to AWS SigV4:
    /// - Auth prefix: `HMAC-SHA256`
    /// - Key derivation seed: bare `secret_key` (not prefixed)
    /// - Timestamp header: `X-Date: YYYYMMDDThhmmssZ`
    /// - API version/action via query params: `?Action={action}&Version={version}`
    /// - Credential scope: `{date}/{region}/{service}/request`
    ///
    /// `method` should be `"GET"` or `"POST"`:
    /// - GET: payload JSON is flattened into query params, body is empty.
    /// - POST: payload JSON is sent as body with `Content-Type: application/json`.
    ///
    /// Per official SDK, only `x-date` is signed.
    async fn signed_request(
        &self,
        service: &str,
        action: &str,
        version: &str,
        payload: &str,
        method: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.signed_request_with_host(service, action, version, payload, method, None)
            .await
    }

    async fn signed_request_with_host(
        &self,
        service: &str,
        action: &str,
        version: &str,
        payload: &str,
        method: &str,
        host_override: Option<&str>,
    ) -> Result<serde_json::Value, AppError> {
        let endpoint = match host_override {
            Some(h) => format!("https://{h}"),
            None => self.endpoint_for_service(service),
        };
        let now = Utc::now();
        let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();

        let is_get = method == "GET";

        // Build query parameters — Action & Version are always present
        let mut query_params: Vec<(String, String)> = vec![
            ("Action".to_string(), action.to_string()),
            ("Version".to_string(), version.to_string()),
        ];

        let body_content = if is_get {
            // For GET: flatten JSON payload into query params, body is empty
            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(map) = obj.as_object() {
                    flatten_to_query_params(map, "", &mut query_params);
                }
            }
            String::new()
        } else {
            // For POST: JSON body
            payload.to_string()
        };

        // Sort query params alphabetically for canonical query string
        query_params.sort_by(|a, b| a.0.cmp(&b.0));
        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        // Step 1: Canonical request
        // Per official SDK: only x-date is signed (no host, no content-type)
        let hashed_payload = sha256_hex(body_content.as_bytes());
        let canonical_headers = format!("x-date:{x_date}\n");
        let signed_headers = "x-date";
        let canonical_request = format!(
            "{method}\n/\n{query_string}\n{canonical_headers}\n{signed_headers}\n{hashed_payload}"
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
            "HMAC-SHA256 Credential={}/{}, SignedHeaders={signed_headers}, Signature={}",
            self.access_key, credential_scope, signature
        );

        let url = format!("{endpoint}/?{query_string}");

        let builder = if is_get {
            self.client.get(&url)
        } else {
            self.client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(body_content)
        };

        let resp = builder
            .header("X-Date", &x_date)
            .header("Authorization", &authorization)
            .send()
            .await?;

        let status = resp.status();

        if !status.is_success() {
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
        self.signed_request(ECS_SERVICE, action, ECS_VERSION, payload, "POST")
            .await
    }

    async fn vpc_request(
        &self,
        action: &str,
        payload: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.signed_request(VPC_SERVICE, action, VPC_VERSION, payload, "GET")
            .await
    }

    async fn ark_request(
        &self,
        action: &str,
        payload: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.signed_request(ARK_SERVICE, action, ARK_VERSION, payload, "POST")
            .await
    }

    /// Generate a temporary ARK API key scoped to the given resource IDs.
    ///
    /// Uses the `GetApiKey` management API to exchange AK/SK credentials for a
    /// scoped bearer token usable with ARK inference endpoints.
    ///
    /// * `resource_type` — `"endpoint"` or `"bot"`
    /// * `resource_ids` — list of endpoint/bot IDs the key is scoped to
    /// * `duration_seconds` — TTL in seconds (max 2,592,000 = 30 days)
    pub async fn get_api_key(
        &self,
        resource_type: &str,
        resource_ids: &[String],
        duration_seconds: u64,
    ) -> Result<(String, u64), AppError> {
        let payload = serde_json::json!({
            "DurationSeconds": duration_seconds,
            "ResourceType": resource_type,
            "ResourceIds": resource_ids,
        });

        let resp = self.ark_request("GetApiKey", &payload.to_string()).await?;

        let result = resp
            .get("Result")
            .ok_or_else(|| AppError::BytePlus("GetApiKey response missing Result field".into()))?;

        let api_key = result
            .get("ApiKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BytePlus("GetApiKey response missing ApiKey".into()))?
            .to_string();

        let expired_time = result
            .get("ExpiredTime")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        Ok((api_key, expired_time))
    }

    /// List ARK inference endpoints.
    ///
    /// Returns the raw Items array from the ListEndpoints response.
    pub async fn list_endpoints(&self) -> Result<Vec<serde_json::Value>, AppError> {
        let ark_host = format!("ark.{}.byteplusapi.com", self.region);
        let payload = serde_json::json!({
            "PageSize": 100,
            "PageNumber": 1,
        });

        let resp = self
            .signed_request_with_host(
                ARK_SERVICE,
                "ListEndpoints",
                ARK_VERSION,
                &payload.to_string(),
                "POST",
                Some(&ark_host),
            )
            .await?;

        let items = resp
            .get("Result")
            .and_then(|r| r.get("Items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(items)
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

        let sg_id = if let Some(sg_id) = resp["Result"]["SecurityGroups"]
            .as_array()
            .and_then(|sgs| sgs.first())
            .and_then(|sg| sg["SecurityGroupId"].as_str())
        {
            sg_id.to_string()
        } else {
            // Create security group
            let create_payload = serde_json::json!({
                "VpcId": vpc_id,
                "SecurityGroupName": "openclaw-sg",
                "Description": "OpenClaw security group",
                "Tags": [{
                    "Key": "app",
                    "Value": "openclaw"
                }]
            });
            let resp = self
                .vpc_request("CreateSecurityGroup", &create_payload.to_string())
                .await?;

            let id = resp["Result"]["SecurityGroupId"]
                .as_str()
                .ok_or_else(|| {
                    AppError::BytePlus(
                        "Missing SecurityGroupId in CreateSecurityGroup response".into(),
                    )
                })?
                .to_string();

            // Wait for security group to become available
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            id
        };

        // Always ensure ingress rules exist (idempotent — duplicates return an error we ignore)
        for (port, desc) in [
            (22, "SSH"),
            (80, "HTTP"),
            (443, "HTTPS"),
            (18789, "OpenClaw Gateway"),
        ] {
            let rule_payload = serde_json::json!({
                "SecurityGroupId": sg_id,
                "Direction": "ingress",
                "Protocol": "tcp",
                "PortStart": port,
                "PortEnd": port,
                "CidrIp": "0.0.0.0/0",
                "Description": desc
            });
            match self
                .vpc_request("AuthorizeSecurityGroupIngress", &rule_payload.to_string())
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    let msg = format!("{e}");
                    // Ignore "already exists" / conflict errors
                    if !msg.contains("SecurityGroupRuleAlreadyExists")
                        && !msg.contains("InvalidSecurityGroupRule.Duplicate")
                        && !msg.contains("InvalidSecurityRule.Conflict")
                        && !msg.contains("already exists")
                        && !msg.contains("Conflict")
                    {
                        return Err(e);
                    }
                }
            }
        }

        Ok(sg_id)
    }

    /// Query DescribeImages to find the latest Ubuntu image.
    async fn find_ubuntu_image(&self) -> Result<String, AppError> {
        let payload = serde_json::json!({
            "OsType": "Linux",
            "Visibility": "public",
            "MaxResults": 100
        });
        let resp = self
            .ecs_request("DescribeImages", &payload.to_string())
            .await?;

        let images = resp["Result"]["Images"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        // Prefer Ubuntu 24.04, then 22.04, then any Ubuntu
        let mut best: Option<(i32, String)> = None;
        for img in &images {
            let name = img["ImageName"].as_str().unwrap_or("");
            let id = img["ImageId"].as_str().unwrap_or("");
            let name_lower = name.to_lowercase();
            if !name_lower.contains("ubuntu") || id.is_empty() {
                continue;
            }

            let priority = if name_lower.contains("24.04") {
                3
            } else if name_lower.contains("22.04") {
                2
            } else if name_lower.contains("20.04") {
                1
            } else {
                0
            };
            if best.as_ref().is_none_or(|(p, _)| priority > *p) {
                best = Some((priority, id.to_string()));
            }
        }

        best.map(|(_, id)| id)
            .ok_or_else(|| AppError::BytePlus("No Ubuntu image found via DescribeImages".into()))
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

        // Find the correct Ubuntu image dynamically
        let image_id = self.find_ubuntu_image().await?;

        let zone_id = format!("{}{DEFAULT_ZONE_SUFFIX}", self.region);

        let payload = serde_json::json!({
            "InstanceName": name,
            "InstanceTypeId": instance_type,
            "ImageId": image_id,
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

    /// List all ECS instances (no tag filter).
    pub async fn list_all_instances(&self) -> Result<Vec<InstanceInfo>, AppError> {
        let payload = serde_json::json!({
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
            "InstanceId": instance_id
        });
        self.ecs_request("DeleteInstance", &payload.to_string())
            .await?;
        Ok(())
    }

    /// Get the EIP allocation ID associated with an instance (if any).
    pub async fn describe_instance_eip(
        &self,
        instance_id: &str,
    ) -> Result<Option<String>, AppError> {
        let payload = serde_json::json!({
            "InstanceIds": [instance_id]
        });
        let resp = self
            .ecs_request("DescribeInstances", &payload.to_string())
            .await?;

        let alloc_id = resp["Result"]["Instances"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|inst| inst["EipAddress"]["AllocationId"].as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(alloc_id)
    }

    /// Disassociate an EIP from its instance.
    pub async fn disassociate_eip(&self, allocation_id: &str) -> Result<(), AppError> {
        let payload = serde_json::json!({
            "AllocationId": allocation_id,
            "InstanceType": "EcsInstance"
        });
        match self
            .vpc_request("DisassociateEipAddress", &payload.to_string())
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("not found")
                    || msg.contains("NotFound")
                    || msg.contains("InvalidEip")
                {
                    Ok(()) // Already disassociated
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Release (delete) an EIP.
    pub async fn release_eip(&self, allocation_id: &str) -> Result<(), AppError> {
        let payload = serde_json::json!({
            "AllocationId": allocation_id
        });
        match self
            .vpc_request("ReleaseEipAddress", &payload.to_string())
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("not found")
                    || msg.contains("NotFound")
                    || msg.contains("InvalidEip")
                {
                    Ok(()) // Already released
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Find the openclaw-tagged VPC and return its ID (if any).
    pub async fn find_openclaw_vpc(&self) -> Result<Option<String>, AppError> {
        let payload = serde_json::json!({
            "MaxResults": 100,
            "TagFilters": [{ "Key": "app", "Values": ["openclaw"] }]
        });
        let resp = self
            .vpc_request("DescribeVpcs", &payload.to_string())
            .await?;

        Ok(resp["Result"]["Vpcs"]
            .as_array()
            .and_then(|vpcs| vpcs.first())
            .and_then(|vpc| vpc["VpcId"].as_str())
            .map(|s| s.to_string()))
    }

    /// Delete a security group by ID (ignores "in use" or "not found" errors).
    pub async fn delete_security_group(&self, sg_id: &str) -> Result<(), AppError> {
        let payload = serde_json::json!({ "SecurityGroupId": sg_id });
        match self
            .vpc_request("DeleteSecurityGroup", &payload.to_string())
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("InUse")
                    || msg.contains("DependencyViolation")
                    || msg.contains("NotFound")
                {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Delete a subnet by ID (ignores "in use" or "not found" errors).
    pub async fn delete_subnet(&self, subnet_id: &str) -> Result<(), AppError> {
        let payload = serde_json::json!({ "SubnetId": subnet_id });
        match self.vpc_request("DeleteSubnet", &payload.to_string()).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("InUse")
                    || msg.contains("DependencyViolation")
                    || msg.contains("NotFound")
                {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Delete a VPC by ID (ignores "in use" or "not found" errors).
    pub async fn delete_vpc(&self, vpc_id: &str) -> Result<(), AppError> {
        let payload = serde_json::json!({ "VpcId": vpc_id });
        match self.vpc_request("DeleteVpc", &payload.to_string()).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("InUse")
                    || msg.contains("DependencyViolation")
                    || msg.contains("NotFound")
                {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Clean up VPC resources (security groups, subnets, then VPC) for an openclaw-tagged VPC.
    /// Errors are logged but do not fail the overall operation (best-effort cleanup).
    pub async fn cleanup_vpc_resources(&self) {
        let vpc_id = match self.find_openclaw_vpc().await {
            Ok(Some(id)) => id,
            _ => return,
        };

        // Delete security groups in the VPC
        let sg_payload = serde_json::json!({
            "VpcId": &vpc_id,
            "MaxResults": 100,
            "TagFilters": [{ "Key": "app", "Values": ["openclaw"] }]
        });
        if let Ok(resp) = self
            .vpc_request("DescribeSecurityGroups", &sg_payload.to_string())
            .await
        {
            if let Some(sgs) = resp["Result"]["SecurityGroups"].as_array() {
                for sg in sgs {
                    if let Some(sg_id) = sg["SecurityGroupId"].as_str() {
                        let _ = self.delete_security_group(sg_id).await;
                    }
                }
            }
        }

        // Delete subnets in the VPC
        let subnet_payload = serde_json::json!({ "VpcId": &vpc_id, "MaxResults": 100 });
        if let Ok(resp) = self
            .vpc_request("DescribeSubnets", &subnet_payload.to_string())
            .await
        {
            if let Some(subnets) = resp["Result"]["Subnets"].as_array() {
                for subnet in subnets {
                    if let Some(subnet_id) = subnet["SubnetId"].as_str() {
                        let _ = self.delete_subnet(subnet_id).await;
                    }
                }
            }
        }

        // Wait for dependent resources to be cleaned up
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        // Delete the VPC itself
        let _ = self.delete_vpc(&vpc_id).await;
    }

    /// Allocate a new Elastic IP address.
    pub async fn allocate_eip(&self) -> Result<(String, String), AppError> {
        let payload = serde_json::json!({
            "BillingType": 2,
            "Bandwidth": 10,
            "Name": "openclaw-eip",
            "Description": "OpenClaw EIP"
        });
        let resp = self
            .vpc_request("AllocateEipAddress", &payload.to_string())
            .await?;
        let allocation_id = resp["Result"]["AllocationId"]
            .as_str()
            .ok_or_else(|| {
                AppError::BytePlus("Missing AllocationId in AllocateEipAddress response".into())
            })?
            .to_string();
        let eip_address = resp["Result"]["EipAddress"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok((allocation_id, eip_address))
    }

    /// Associate an EIP with an ECS instance.
    pub async fn associate_eip(
        &self,
        allocation_id: &str,
        instance_id: &str,
    ) -> Result<(), AppError> {
        let payload = serde_json::json!({
            "AllocationId": allocation_id,
            "InstanceId": instance_id,
            "InstanceType": "EcsInstance"
        });
        self.vpc_request("AssociateEipAddress", &payload.to_string())
            .await?;
        Ok(())
    }

    /// Poll until instance is RUNNING, then allocate and associate an EIP.
    pub async fn wait_for_running(
        &self,
        instance_id: &str,
        timeout: std::time::Duration,
    ) -> Result<InstanceInfo, AppError> {
        let start = std::time::Instant::now();

        // First wait for instance to be RUNNING
        loop {
            if start.elapsed() > timeout {
                return Err(AppError::Timeout(
                    "BytePlus ECS instance to become RUNNING".into(),
                ));
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            match self.describe_instance(instance_id).await {
                Ok(info) if info.status == "RUNNING" => break,
                Ok(_) => continue,
                Err(_) => continue,
            }
        }

        // Allocate and associate an EIP for public internet access
        let (allocation_id, eip_address) = self.allocate_eip().await?;

        // Wait a moment for EIP to be ready
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        self.associate_eip(&allocation_id, instance_id).await?;

        // Wait for the EIP to appear on the instance
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Re-describe to get the updated info with the EIP
        let mut info = self.describe_instance(instance_id).await?;
        if info.public_ip.is_none() || info.public_ip.as_deref() == Some("") {
            // Use the EIP address we just allocated
            info.public_ip = Some(eip_address);
        }

        Ok(info)
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
