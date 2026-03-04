use crate::error::AppError;
use crate::cloud_provider::{CloudProvider, CreateInstanceParams, InstanceInfo, KeyInfo};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub struct TencentClient {
    client: Client,
    secret_id: String,
    secret_key: String,
    region: String,
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

    // NOTE: TC3 signing and full API surface is non-trivial. These methods are
    // placeholders that mirror the DoClient API shape. Implementing full TC3
    // signing is left as a follow-up; these methods should perform signed
    // requests to the Tencent Cloud CVM API (cvm.tencentcloudapi.com).

    async fn sign_and_post<T: Serialize + ?Sized>(
        &self,
        _action: &str,
        _body: &T,
    ) -> Result<serde_json::Value, AppError> {
        // Placeholder: construct signed request per TC3-HMAC-SHA256
        Err(AppError::Generic("Tencent signing not implemented".into()))
    }
}

#[async_trait::async_trait]
impl CloudProvider for TencentClient {
    async fn upload_ssh_key(&self, name: &str, public_key: &str) -> Result<KeyInfo, AppError> {
        // Tencent has a KeyPair API; return a compatible KeyInfo.id
        let _ = (name, public_key);
        Err(AppError::Generic("upload_ssh_key not implemented for Tencent".into()))
    }

    async fn delete_ssh_key(&self, key_id: &str) -> Result<(), AppError> {
        let _ = key_id;
        Err(AppError::Generic("delete_ssh_key not implemented for Tencent".into()))
    }

    async fn create_instance(&self, params: CreateInstanceParams) -> Result<InstanceInfo, AppError> {
        let _ = params;
        Err(AppError::Generic("create_instance not implemented for Tencent".into()))
    }

    async fn wait_for_active(&self, instance_id: &str, _timeout_secs: u64) -> Result<InstanceInfo, AppError> {
        let _ = instance_id;
        Err(AppError::Generic("wait_for_active not implemented for Tencent".into()))
    }

    async fn delete_instance(&self, instance_id: &str) -> Result<(), AppError> {
        let _ = instance_id;
        Err(AppError::Generic("delete_instance not implemented for Tencent".into()))
    }

    async fn list_instances(&self, _tag: &str) -> Result<Vec<InstanceInfo>, AppError> {
        Err(AppError::Generic("list_instances not implemented for Tencent".into()))
    }
}
