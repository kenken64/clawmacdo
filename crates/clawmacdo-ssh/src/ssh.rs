use clawmacdo_core::config;
use clawmacdo_core::error::AppError;
use ssh2::Session;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use std::collections::HashMap;
use std::fs;

/// Generated SSH key pair paths
pub struct KeyPair {
    pub private_key_path: PathBuf,
    pub public_key_openssh: String,
}

/// Generate an RSA-4096 SSH key pair (PEM format) via ssh-keygen.
///
/// PEM-format RSA keys are used because libssh2 (the C library behind the
/// `ssh2` crate) does not reliably support Ed25519 keys on Windows.
/// GGenerate keypair.
pub fn generate_keypair(deploy_id: &str) -> Result<KeyPair, AppError> {
    let keys_dir = config::keys_dir()?;
    std::fs::create_dir_all(&keys_dir)?;

    let private_path = keys_dir.join(format!("clawmacdo_{deploy_id}"));
    let pub_path = keys_dir.join(format!("clawmacdo_{deploy_id}.pub"));

    // Generate RSA key in PEM format (universally supported by libssh2)
    let status = std::process::Command::new("ssh-keygen")
        .args(["-t", "rsa", "-b", "4096", "-m", "PEM", "-f"])
        .arg(&private_path)
        .args(["-N", "", "-q"])
        .status()
        .map_err(|e| AppError::SshKeyGen(format!("Failed to run ssh-keygen: {e}")))?;

    if !status.success() {
        return Err(AppError::SshKeyGen(
            "ssh-keygen exited with non-zero status".into(),
        ));
    }

    let public_openssh = std::fs::read_to_string(&pub_path)
        .map_err(|e| AppError::SshKeyGen(format!("Failed to read public key: {e}")))?
        .trim()
        .to_string();

    // Attempt to set file permissions to 600 on Unix-like systems
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&private_path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(KeyPair {
        private_key_path: private_path,
        public_key_openssh: public_openssh,
    })
}

/// Load known host keys from `~/.clawmacdo/known_hosts`.
/// Returns a map of `ip -> (base64_key, key_type_name)`.
fn load_known_hosts() -> Result<HashMap<String, (String, String)>, AppError> {
    let path = config::known_hosts_path()?;
    let mut map = HashMap::new();
    if let Ok(contents) = fs::read_to_string(&path) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() == 3 {
                map.insert(
                    parts[0].to_string(),
                    (parts[1].to_string(), parts[2].to_string()),
                );
            }
        }
    }
    Ok(map)
}

/// Save a host key to `~/.clawmacdo/known_hosts` (TOFU).
fn save_known_host(ip: &str, key_b64: &str, key_type: &str) -> Result<(), AppError> {
    let path = config::known_hosts_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let entry = format!("{ip} {key_b64} {key_type}\n");
    // Atomic-ish append: read existing, append, write back
    let mut contents = fs::read_to_string(&path).unwrap_or_default();
    contents.push_str(&entry);
    fs::write(&path, contents)?;
    Ok(())
}

/// Remove a known host entry (e.g. when a server is destroyed and IP may be reused).
pub fn remove_known_host(ip: &str) -> Result<(), AppError> {
    let path = config::known_hosts_path()?;
    if let Ok(contents) = fs::read_to_string(&path) {
        let filtered: String = contents
            .lines()
            .filter(|line| !line.starts_with(&format!("{ip} ")))
            .map(|line| format!("{line}\n"))
            .collect();
        fs::write(&path, filtered)?;
    }
    Ok(())
}

/// Verify the remote host key using Trust On First Use (TOFU).
/// On first connection to an IP, the key is saved. On subsequent connections,
/// the key is compared — a mismatch returns an error.
fn verify_host_key(sess: &Session, ip: &str) -> Result<(), AppError> {
    let (key_bytes, key_type) = sess
        .host_key()
        .ok_or_else(|| AppError::Ssh(format!("No host key returned by {ip}")))?;

    let key_b64 = BASE64.encode(key_bytes);
    let key_type_name = format!("{key_type:?}");

    let known = load_known_hosts()?;
    if let Some((stored_b64, _stored_type)) = known.get(ip) {
        if *stored_b64 != key_b64 {
            return Err(AppError::HostKeyMismatch {
                ip: ip.to_string(),
                expected: stored_b64.clone(),
                actual: key_b64,
            });
        }
    } else {
        save_known_host(ip, &key_b64, &key_type_name)?;
    }
    Ok(())
}

/// Connect to a remote host via SSH using a private key file.
fn connect(ip: &str, private_key_path: &Path) -> Result<Session, AppError> {
    connect_as(ip, private_key_path, "root")
}

fn connect_as(ip: &str, private_key_path: &Path, username: &str) -> Result<Session, AppError> {
    let addr = format!("{ip}:22");
    let sock_addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| AppError::Ssh(format!("Invalid address {addr}: {e}")))?;

    // Use connect_timeout to avoid long hangs on Windows (default TCP timeout is ~21s)
    let tcp = TcpStream::connect_timeout(&sock_addr, std::time::Duration::from_secs(10))
        .map_err(|e| AppError::Ssh(format!("TCP connect to {ip}: {e}")))?;

    // Enable TCP keepalive to detect silently dropped connections (e.g. after ufw reload).
    // Without this, read_to_string can block for 300s on a dead connection.
    let sock = socket2::SockRef::from(&tcp);
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(std::time::Duration::from_secs(15))
        .with_interval(std::time::Duration::from_secs(5));
    let _ = sock.set_tcp_keepalive(&keepalive);

    // Keep command I/O timeout long enough for package installs and service setup.
    let _ = tcp.set_read_timeout(Some(std::time::Duration::from_secs(300)));
    let _ = tcp.set_write_timeout(Some(std::time::Duration::from_secs(300)));

    let mut sess = Session::new().map_err(|e| AppError::Ssh(format!("Session::new: {e}")))?;
    sess.set_timeout(300_000); // 5 min timeout for SSH-level operations
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| AppError::Ssh(format!("SSH handshake with {ip}: {e}")))?;

    // Verify host key before sending credentials (TOFU)
    verify_host_key(&sess, ip)?;

    sess.userauth_pubkey_file(username, None, private_key_path, None)
        .map_err(|e| AppError::Ssh(format!("SSH auth to {ip}: {e}")))?;

    Ok(sess)
}

fn read_command_output(mut channel: ssh2::Channel) -> Result<String, AppError> {
    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|e| AppError::Ssh(format!("Read output: {e}")))?;

    channel
        .wait_close()
        .map_err(|e| AppError::Ssh(format!("Wait close: {e}")))?;

    let exit_status = channel.exit_status().unwrap_or(-1);
    if exit_status != 0 {
        let trimmed = output.trim();
        let details = if !trimmed.is_empty() {
            trimmed.to_string()
        } else {
            "no output captured".to_string()
        };
        return Err(AppError::Ssh(format!(
            "Command exited with status {exit_status}: {details}"
        )));
    }

    Ok(output)
}

pub fn exec_with_input_as(
    ip: &str,
    private_key_path: &Path,
    command: &str,
    input: &[u8],
    username: &str,
) -> Result<String, AppError> {
    let sess = connect_as(ip, private_key_path, username)?;
    let mut channel = sess
        .channel_session()
        .map_err(|e| AppError::Ssh(format!("Open channel: {e}")))?;
    channel
        .exec(command)
        .map_err(|e| AppError::Ssh(format!("Exec command: {e}")))?;
    channel
        .write_all(input)
        .map_err(|e| AppError::Ssh(format!("Write stdin: {e}")))?;
    channel
        .send_eof()
        .map_err(|e| AppError::Ssh(format!("Send EOF: {e}")))?;

    read_command_output(channel)
}

pub fn exec_with_input(
    ip: &str,
    private_key_path: &Path,
    command: &str,
    input: &[u8],
) -> Result<String, AppError> {
    exec_with_input_as(ip, private_key_path, command, input, "root")
}

/// Execute a command on the remote host as a specific user.
pub fn exec_as(
    ip: &str,
    private_key_path: &Path,
    command: &str,
    username: &str,
) -> Result<String, AppError> {
    let sess = connect_as(ip, private_key_path, username)?;
    let mut channel = sess
        .channel_session()
        .map_err(|e| AppError::Ssh(format!("Open channel: {e}")))?;
    channel
        .exec(&format!("{{ {command}\n}} 2>&1"))
        .map_err(|e| AppError::Ssh(format!("Exec command: {e}")))?;

    read_command_output(channel)
}

/// Execute a command on the remote host and return stdout.
/// EExec.
pub fn exec(ip: &str, private_key_path: &Path, command: &str) -> Result<String, AppError> {
    let sess = connect(ip, private_key_path)?;
    let mut channel = sess
        .channel_session()
        .map_err(|e| AppError::Ssh(format!("Open channel: {e}")))?;

    // Merge stderr into stdout to avoid read deadlock.
    // libssh2 read_to_string(stdout) blocks if remote wrote to stderr
    // and the SSH window fills up, causing a deadlock.
    // Use \n before } so heredocs inside the command don't break bash parsing.
    channel
        .exec(&format!("{{ {command}\n}} 2>&1"))
        .map_err(|e| AppError::Ssh(format!("Exec command: {e}")))?;

    read_command_output(channel)
}

/// Upload a local file to the remote host via SCP as a specific user.
pub fn scp_upload_as(
    ip: &str,
    private_key_path: &Path,
    local_path: &Path,
    remote_path: &str,
    username: &str,
) -> Result<(), AppError> {
    let sess = connect_as(ip, private_key_path, username)?;

    let metadata = std::fs::metadata(local_path)?;
    let file_size = metadata.len();

    let mut remote_file = sess
        .scp_send(Path::new(remote_path), 0o644, file_size, None)
        .map_err(|e| AppError::Ssh(format!("SCP send init: {e}")))?;

    let local_data = std::fs::read(local_path)?;
    remote_file
        .write_all(&local_data)
        .map_err(|e| AppError::Ssh(format!("SCP write: {e}")))?;

    // Signal EOF
    remote_file
        .send_eof()
        .map_err(|e| AppError::Ssh(format!("SCP send_eof: {e}")))?;
    remote_file
        .wait_eof()
        .map_err(|e| AppError::Ssh(format!("SCP wait_eof: {e}")))?;
    remote_file
        .close()
        .map_err(|e| AppError::Ssh(format!("SCP close: {e}")))?;
    remote_file
        .wait_close()
        .map_err(|e| AppError::Ssh(format!("SCP wait_close: {e}")))?;

    Ok(())
}

/// Upload a local file to the remote host via SCP.
/// SScp upload.
pub fn scp_upload(
    ip: &str,
    private_key_path: &Path,
    local_path: &Path,
    remote_path: &str,
) -> Result<(), AppError> {
    let sess = connect(ip, private_key_path)?;

    let metadata = std::fs::metadata(local_path)?;
    let file_size = metadata.len();

    let mut remote_file = sess
        .scp_send(Path::new(remote_path), 0o644, file_size, None)
        .map_err(|e| AppError::Ssh(format!("SCP send init: {e}")))?;

    let local_data = std::fs::read(local_path)?;
    remote_file
        .write_all(&local_data)
        .map_err(|e| AppError::Ssh(format!("SCP write: {e}")))?;

    // Signal EOF
    remote_file
        .send_eof()
        .map_err(|e| AppError::Ssh(format!("SCP send_eof: {e}")))?;
    remote_file
        .wait_eof()
        .map_err(|e| AppError::Ssh(format!("SCP wait_eof: {e}")))?;
    remote_file
        .close()
        .map_err(|e| AppError::Ssh(format!("SCP close: {e}")))?;
    remote_file
        .wait_close()
        .map_err(|e| AppError::Ssh(format!("SCP wait_close: {e}")))?;

    Ok(())
}

/// Upload in-memory bytes to the remote host via SCP as a specific user.
/// Avoids writing to a local temp file.
pub fn scp_upload_bytes(
    ip: &str,
    private_key_path: &Path,
    data: &[u8],
    remote_path: &str,
    mode: i32,
    username: &str,
) -> Result<(), AppError> {
    let sess = connect_as(ip, private_key_path, username)?;

    let mut remote_file = sess
        .scp_send(Path::new(remote_path), mode, data.len() as u64, None)
        .map_err(|e| AppError::Ssh(format!("SCP send init: {e}")))?;

    remote_file
        .write_all(data)
        .map_err(|e| AppError::Ssh(format!("SCP write: {e}")))?;

    remote_file
        .send_eof()
        .map_err(|e| AppError::Ssh(format!("SCP send_eof: {e}")))?;
    remote_file
        .wait_eof()
        .map_err(|e| AppError::Ssh(format!("SCP wait_eof: {e}")))?;
    remote_file
        .close()
        .map_err(|e| AppError::Ssh(format!("SCP close: {e}")))?;
    remote_file
        .wait_close()
        .map_err(|e| AppError::Ssh(format!("SCP wait_close: {e}")))?;

    Ok(())
}

/// Download a file from the remote host via SCP.
/// SScp download.
pub fn scp_download(
    ip: &str,
    private_key_path: &Path,
    remote_path: &str,
    local_path: &Path,
) -> Result<(), AppError> {
    let sess = connect(ip, private_key_path)?;

    let (mut remote_file, _stat) = sess
        .scp_recv(Path::new(remote_path))
        .map_err(|e| AppError::Ssh(format!("SCP recv init: {e}")))?;

    let mut contents = Vec::new();
    remote_file
        .read_to_end(&mut contents)
        .map_err(|e| AppError::Ssh(format!("SCP read: {e}")))?;

    std::fs::write(local_path, &contents)?;
    Ok(())
}

/// Wait for SSH to accept connections (retries every 5s).
/// WWait for ssh.
pub async fn wait_for_ssh(
    ip: &str,
    private_key_path: &Path,
    timeout: std::time::Duration,
) -> Result<(), AppError> {
    let start = std::time::Instant::now();
    let key = private_key_path.to_path_buf();
    let ip = ip.to_string();
    loop {
        if start.elapsed() > timeout {
            return Err(AppError::Timeout("SSH to accept connections".into()));
        }
        let ip_clone = ip.clone();
        let key_clone = key.clone();
        let result = tokio::task::spawn_blocking(move || {
            // Try root first, then ubuntu (Tencent Cloud default user)
            exec(&ip_clone, &key_clone, "echo ok")
                .or_else(|_| exec_as(&ip_clone, &key_clone, "echo ok", "ubuntu"))
        })
        .await;

        match result {
            Ok(Ok(_)) => return Ok(()),
            _ => {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

/// Wait for the cloud-init sentinel file to appear on the remote host.
/// When `ssh_user` is provided (e.g. "ubuntu" for Lightsail), connects as that
/// user and prefixes commands with `sudo` so we can check the root-owned sentinel.
pub async fn wait_for_cloud_init(
    ip: &str,
    private_key_path: &Path,
    timeout: std::time::Duration,
    ssh_user: Option<&str>,
) -> Result<(), AppError> {
    let start = std::time::Instant::now();
    let sentinel = config::CLOUD_INIT_SENTINEL;
    let key = private_key_path.to_path_buf();
    let ip = ip.to_string();
    let ssh_user = ssh_user.map(|s| s.to_string());
    let mut last_status = String::from("unknown");
    loop {
        if start.elapsed() > timeout {
            let ip_clone = ip.clone();
            let key_clone = key.clone();
            let user_clone = ssh_user.clone();
            let diag_cmd_str = format!(
                "{}(cloud-init status --long 2>/dev/null || cloud-init status 2>/dev/null || true); \
                 echo '--- cloud-init.log (tail) ---'; \
                 (tail -n 30 /var/log/cloud-init.log 2>/dev/null || true); \
                 echo '--- cloud-init-output.log (tail) ---'; \
                 (tail -n 30 /var/log/cloud-init-output.log 2>/dev/null || true)",
                if user_clone.as_deref().is_some_and(|u| u != "root") { "sudo " } else { "" }
            );
            let diagnostics =
                match tokio::task::spawn_blocking(move || match user_clone.as_deref() {
                    Some(u) if u != "root" => exec_as(&ip_clone, &key_clone, &diag_cmd_str, u),
                    _ => exec(&ip_clone, &key_clone, &diag_cmd_str),
                })
                .await
                {
                    Ok(Ok(out)) if !out.trim().is_empty() => out,
                    _ => "No diagnostic output available".to_string(),
                };

            return Err(AppError::Timeout(format!(
                "cloud-init to complete (last status: {last_status})\n{diagnostics}"
            )));
        }
        let ip_clone = ip.clone();
        let key_clone = key.clone();
        let user_clone = ssh_user.clone();
        let sudo_prefix = if user_clone.as_deref().is_some_and(|u| u != "root") {
            "sudo "
        } else {
            ""
        };
        let cmd = format!(
            "if {sudo_prefix}test -f {sentinel}; then echo done; else cloud-init status 2>/dev/null || echo pending; fi"
        );
        let result = tokio::task::spawn_blocking(move || match user_clone.as_deref() {
            Some(u) if u != "root" => exec_as(&ip_clone, &key_clone, &cmd, u),
            _ => exec(&ip_clone, &key_clone, &cmd),
        })
        .await;

        match result {
            Ok(Ok(out)) if out.trim() == "done" => return Ok(()),
            Ok(Ok(out)) => {
                let trimmed = out.trim();
                if !trimmed.is_empty() {
                    last_status = trimmed.to_string();
                }
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
            _ => {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        }
    }
}
