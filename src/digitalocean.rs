use crate::error::AppError;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

const API_BASE: &str = "https://api.digitalocean.com/v2";

pub struct DoClient {
    client: reqwest::Client,
}

// --- Request / Response types ---

#[derive(Serialize)]
struct CreateKeyRequest<'a> {
    name: &'a str,
    public_key: &'a str,
}

#[derive(Deserialize)]
struct CreateKeyResponse {
    ssh_key: SshKeyInfo,
}

#[derive(Deserialize)]
pub struct SshKeyInfo {
    pub id: u64,
    pub fingerprint: String,
}

#[derive(Serialize)]
struct CreateDropletRequest<'a> {
    name: &'a str,
    region: &'a str,
    size: &'a str,
    image: &'a str,
    ssh_keys: Vec<u64>,
    user_data: &'a str,
    tags: Vec<&'a str>,
    backups: bool,
}

#[derive(Deserialize)]
struct CreateDropletResponse {
    droplet: DropletInfo,
}

#[derive(Debug, Deserialize)]
struct GetDropletResponse {
    droplet: DropletInfo,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct DropletInfo {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub networks: Networks,
    pub region: RegionInfo,
    pub size_slug: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Networks {
    pub v4: Vec<NetworkV4>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkV4 {
    pub ip_address: String,
    #[serde(rename = "type")]
    pub net_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct RegionInfo {
    pub slug: String,
    pub name: String,
}

#[derive(Deserialize)]
struct ListDropletsResponse {
    droplets: Vec<DropletInfo>,
}

impl DropletInfo {
    pub fn public_ip(&self) -> Option<String> {
        self.networks
            .v4
            .iter()
            .find(|n| n.net_type == "public")
            .map(|n| n.ip_address.clone())
    }
}

impl DoClient {
    pub fn new(token: &str) -> Result<Self, AppError> {
        let mut headers = HeaderMap::new();
        let auth_value = format!("Bearer {}", token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value)
                .map_err(|e| AppError::DigitalOcean(format!("Invalid token: {e}")))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self { client })
    }

    /// Upload an SSH public key to DigitalOcean. Returns key ID and fingerprint.
    pub async fn upload_ssh_key(
        &self,
        name: &str,
        public_key: &str,
    ) -> Result<SshKeyInfo, AppError> {
        let body = CreateKeyRequest { name, public_key };
        let resp = self
            .client
            .post(format!("{API_BASE}/account/keys"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::DigitalOcean(format!(
                "Upload SSH key failed ({status}): {text}"
            )));
        }

        let parsed: CreateKeyResponse = resp.json().await?;
        Ok(parsed.ssh_key)
    }

    /// Create a droplet with the given parameters and cloud-init user data.
    pub async fn create_droplet(
        &self,
        name: &str,
        region: &str,
        size: &str,
        ssh_key_id: u64,
        user_data: &str,
        enable_backups: bool,
    ) -> Result<DropletInfo, AppError> {
        let body = CreateDropletRequest {
            name,
            region,
            size,
            image: "ubuntu-24-04-x64",
            ssh_keys: vec![ssh_key_id],
            user_data,
            tags: vec![crate::config::DROPLET_TAG],
            backups: enable_backups,
        };

        let resp = self
            .client
            .post(format!("{API_BASE}/droplets"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::DigitalOcean(format!(
                "Create droplet failed ({status}): {text}"
            )));
        }

        let parsed: CreateDropletResponse = resp.json().await?;
        Ok(parsed.droplet)
    }

    /// Poll until the droplet reaches "active" status. Returns updated DropletInfo.
    pub async fn wait_for_active(
        &self,
        droplet_id: u64,
        timeout: std::time::Duration,
    ) -> Result<DropletInfo, AppError> {
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err(AppError::Timeout("droplet to become active".into()));
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            let droplet = self.get_droplet(droplet_id).await?;
            if droplet.status == "active" && droplet.public_ip().is_some() {
                return Ok(droplet);
            }
        }
    }

    /// Get a single droplet by ID.
    pub async fn get_droplet(&self, droplet_id: u64) -> Result<DropletInfo, AppError> {
        let resp = self
            .client
            .get(format!("{API_BASE}/droplets/{droplet_id}"))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::DigitalOcean(format!(
                "Get droplet failed ({status}): {text}"
            )));
        }

        let parsed: GetDropletResponse = resp.json().await?;
        Ok(parsed.droplet)
    }

    /// List all droplets tagged with "openclaw".
    pub async fn list_droplets(&self) -> Result<Vec<DropletInfo>, AppError> {
        let resp = self
            .client
            .get(format!(
                "{API_BASE}/droplets?tag_name={}",
                crate::config::DROPLET_TAG
            ))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::DigitalOcean(format!(
                "List droplets failed ({status}): {text}"
            )));
        }

        let parsed: ListDropletsResponse = resp.json().await?;
        Ok(parsed.droplets)
    }
}
