use crate::error::AppError;
use crate::ssh;
use std::path::Path;

/// Execute a command on the remote host as root.
pub fn ssh_root(ip: &str, key: &Path, cmd: &str) -> Result<String, AppError> {
    ssh::exec(ip, key, cmd)
}

/// Execute a command on the remote host as root, wrapped in spawn_blocking.
pub async fn ssh_root_async(ip: &str, key: &Path, cmd: &str) -> Result<String, AppError> {
    let ip = ip.to_string();
    let key = key.to_path_buf();
    let cmd = cmd.to_string();
    tokio::task::spawn_blocking(move || ssh_root(&ip, &key, &cmd))
        .await
        .map_err(|e| AppError::Ssh(format!("spawn_blocking join: {e}")))?
}

/// Execute a command on the remote host as the openclaw user via root SSH.
/// Uses `su - openclaw -c '...'` so we only need root's SSH key.
pub fn ssh_as_openclaw(ip: &str, key: &Path, cmd: &str) -> Result<String, AppError> {
    let wrapped = format!(
        "su - openclaw -c {}",
        shell_escape(cmd)
    );
    ssh::exec(ip, key, &wrapped)
}

/// Execute a command as openclaw user, wrapped in spawn_blocking.
pub async fn ssh_as_openclaw_async(
    ip: &str,
    key: &Path,
    cmd: &str,
) -> Result<String, AppError> {
    let ip = ip.to_string();
    let key = key.to_path_buf();
    let cmd = cmd.to_string();
    tokio::task::spawn_blocking(move || ssh_as_openclaw(&ip, &key, &cmd))
        .await
        .map_err(|e| AppError::Ssh(format!("spawn_blocking join: {e}")))?
}

/// Shell-escape a string for use in `su -c '...'`.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
