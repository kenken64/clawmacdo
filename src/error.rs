use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum AppError {
    #[error("Home directory not found")]
    HomeDirNotFound,

    #[error("Backup failed: {0}")]
    Backup(String),

    #[error("No backups found in {0}")]
    NoBackups(String),

    #[error("DigitalOcean API error: {0}")]
    DigitalOcean(String),

    #[error("SSH error: {0}")]
    Ssh(String),

    #[error("SSH key generation error: {0}")]
    SshKeyGen(String),

    #[error("Timeout waiting for {0}")]
    Timeout(String),

    #[error("Cloud-init generation error: {0}")]
    CloudInit(String),

    #[error("Provision error ({phase}): {message}")]
    Provision { phase: String, message: String },

    #[error("Missing required parameter: {0}")]
    MissingParam(String),

    #[error("Deploy failed at step {step}: {message}")]
    DeployFailed { step: u32, message: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}
