use clawmacdo_core::error::AppError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyInfo {
    pub id: String,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub public_ip: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CreateInstanceParams {
    pub name: String,
    pub region: String,
    pub size: String,
    pub image: String,
    pub ssh_key_id: String,
    pub user_data: String,
    pub tags: Vec<String>,
    pub customer_email: String,
}

#[allow(dead_code)]
#[async_trait]
pub trait CloudProvider: Send + Sync {
    async fn upload_ssh_key(&self, name: &str, public_key: &str) -> Result<KeyInfo, AppError>;
    async fn delete_ssh_key(&self, key_id: &str) -> Result<(), AppError>;
    async fn create_instance(&self, params: CreateInstanceParams)
        -> Result<InstanceInfo, AppError>;
    async fn wait_for_active(
        &self,
        instance_id: &str,
        timeout_secs: u64,
    ) -> Result<InstanceInfo, AppError>;
    async fn delete_instance(&self, instance_id: &str) -> Result<(), AppError>;
    async fn list_instances(&self, tag: &str) -> Result<Vec<InstanceInfo>, AppError>;
}
