use clawmacdo_core::error::AppError;
use clawmacdo_ssh as ssh;
use std::path::Path;

fn ssh_with_stdin_as(
    ip: &str,
    key: &Path,
    remote_command: &str,
    script: &str,
    ssh_user: &str,
) -> Result<String, AppError> {
    ssh::exec_with_input_as(ip, key, remote_command, script.as_bytes(), ssh_user)
}

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
        ssh_with_stdin_as(ip, key, "sudo /bin/bash -se", cmd, ssh_user)
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
/// Uses a stdin-fed shell so command contents are not re-quoted through `su -c`.
/// SSsh as openclaw.
pub fn ssh_as_openclaw(ip: &str, key: &Path, cmd: &str) -> Result<String, AppError> {
    ssh::exec_with_input(
        ip,
        key,
        "su - openclaw -s /bin/bash -c '/bin/bash -se'",
        cmd.as_bytes(),
    )
}

/// Execute a command as the openclaw user using the specified SSH user.
/// If ssh_user is "root", uses `su - openclaw`; otherwise uses `sudo su - openclaw`.
pub fn ssh_as_openclaw_with_user(
    ip: &str,
    key: &Path,
    cmd: &str,
    ssh_user: &str,
) -> Result<String, AppError> {
    if ssh_user == "root" {
        ssh::exec_with_input(
            ip,
            key,
            "su - openclaw -s /bin/bash -c '/bin/bash -se'",
            cmd.as_bytes(),
        )
    } else {
        ssh_with_stdin_as(
            ip,
            key,
            "sudo su - openclaw -s /bin/bash -c '/bin/bash -se'",
            cmd,
            ssh_user,
        )
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
