use clawmacdo_core::error::AppError;
use clawmacdo_ssh as ssh;
use std::path::Path;

/// Execute a command on the remote host with root privileges.
/// When ssh_user is "root", runs the command directly.
/// Otherwise, connects as the given user and wraps the command with `sudo`.
pub fn ssh_root(ip: &str, key: &Path, cmd: &str) -> Result<String, AppError> {
    ssh::exec(ip, key, cmd)
}

/// Execute a root-level command using the specified SSH user.
/// If the user is "root", runs directly; otherwise wraps with `sudo`.
pub fn ssh_root_as(ip: &str, key: &Path, cmd: &str, ssh_user: &str) -> Result<String, AppError> {
    if ssh_user == "root" {
        ssh::exec(ip, key, cmd)
    } else {
        // Wrap entire command in sudo bash -c so pipes, ||, && all run as root
        let escaped = cmd.replace('\'', "'\\''");
        let sudo_cmd = format!("sudo bash -c '{escaped}'");
        ssh::exec_as(ip, key, &sudo_cmd, ssh_user)
    }
}

/// Execute a command on the remote host as root, wrapped in spawn_blocking.
/// SSsh root async.
pub async fn ssh_root_async(ip: &str, key: &Path, cmd: &str) -> Result<String, AppError> {
    let ip = ip.to_string();
    let key = key.to_path_buf();
    let cmd = cmd.to_string();
    tokio::task::spawn_blocking(move || ssh_root(&ip, &key, &cmd))
        .await
        .map_err(|e| AppError::Ssh(format!("spawn_blocking join: {e}")))?
}

/// Async version of ssh_root_as.
pub async fn ssh_root_as_async(
    ip: &str,
    key: &Path,
    cmd: &str,
    ssh_user: &str,
) -> Result<String, AppError> {
    let ip = ip.to_string();
    let key = key.to_path_buf();
    let cmd = cmd.to_string();
    let ssh_user = ssh_user.to_string();
    tokio::task::spawn_blocking(move || ssh_root_as(&ip, &key, &cmd, &ssh_user))
        .await
        .map_err(|e| AppError::Ssh(format!("spawn_blocking join: {e}")))?
}

/// Execute a command on the remote host as the openclaw user via root SSH.
/// Uses `su - openclaw -c '...'` so we only need root's SSH key.
/// SSsh as openclaw.
pub fn ssh_as_openclaw(ip: &str, key: &Path, cmd: &str) -> Result<String, AppError> {
    let wrapped = format!("su - openclaw -c {}", shell_escape(cmd));
    ssh::exec(ip, key, &wrapped)
}

/// Execute a command as the openclaw user using the specified SSH user.
/// If ssh_user is "root", uses `su - openclaw`; otherwise uses `sudo su - openclaw`.
pub fn ssh_as_openclaw_with_user(
    ip: &str,
    key: &Path,
    cmd: &str,
    ssh_user: &str,
) -> Result<String, AppError> {
    let escaped = shell_escape(cmd);
    if ssh_user == "root" {
        let wrapped = format!("su - openclaw -c {escaped}");
        ssh::exec(ip, key, &wrapped)
    } else {
        let wrapped = format!("sudo su - openclaw -c {escaped}");
        ssh::exec_as(ip, key, &wrapped, ssh_user)
    }
}

/// Execute a command as openclaw user, wrapped in spawn_blocking.
/// SSsh as openclaw async.
pub async fn ssh_as_openclaw_async(ip: &str, key: &Path, cmd: &str) -> Result<String, AppError> {
    let ip = ip.to_string();
    let key = key.to_path_buf();
    let cmd = cmd.to_string();
    tokio::task::spawn_blocking(move || ssh_as_openclaw(&ip, &key, &cmd))
        .await
        .map_err(|e| AppError::Ssh(format!("spawn_blocking join: {e}")))?
}

/// Async version of ssh_as_openclaw_with_user.
pub async fn ssh_as_openclaw_with_user_async(
    ip: &str,
    key: &Path,
    cmd: &str,
    ssh_user: &str,
) -> Result<String, AppError> {
    let ip = ip.to_string();
    let key = key.to_path_buf();
    let cmd = cmd.to_string();
    let ssh_user = ssh_user.to_string();
    tokio::task::spawn_blocking(move || ssh_as_openclaw_with_user(&ip, &key, &cmd, &ssh_user))
        .await
        .map_err(|e| AppError::Ssh(format!("spawn_blocking join: {e}")))?
}

/// Shell-escape a string for use in `su -c '...'`.
/// SShell escape.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
