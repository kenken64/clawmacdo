use crate::config;
use crate::error::AppError;
use ssh2::Session;
use std::io::Read;
use std::net::TcpStream;
use std::path::{Path, PathBuf};

/// Generated SSH key pair paths
pub struct KeyPair {
    pub private_key_path: PathBuf,
    pub public_key_openssh: String,
}

/// Generate an RSA-4096 SSH key pair (PEM format) via ssh-keygen.
///
/// PEM-format RSA keys are used because libssh2 (the C library behind the
/// `ssh2` crate) does not reliably support Ed25519 keys on Windows.
pub fn generate_keypair(deploy_id: &str) -> Result<KeyPair, AppError> {
    let keys_dir = config::keys_dir()?;
    std::fs::create_dir_all(&keys_dir)?;

    let private_path = keys_dir.join(format!("clawmacdo_{deploy_id}"));
    let pub_path = keys_dir.join(format!("clawmacdo_{deploy_id}.pub"));

    // Generate RSA key in PEM format (universally supported by libssh2)
    let status = std::process::Command::new("ssh-keygen")
        .args([
            "-t", "rsa",
            "-b", "4096",
            "-m", "PEM",
            "-f",
        ])
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

/// Connect to a remote host via SSH using a private key file.
fn connect(ip: &str, private_key_path: &Path) -> Result<Session, AppError> {
    let addr = format!("{ip}:22");
    let sock_addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| AppError::Ssh(format!("Invalid address {addr}: {e}")))?;

    // Use connect_timeout to avoid long hangs on Windows (default TCP timeout is ~21s)
    let tcp = TcpStream::connect_timeout(&sock_addr, std::time::Duration::from_secs(10))
        .map_err(|e| AppError::Ssh(format!("TCP connect to {ip}: {e}")))?;

    // Set read/write timeouts so SSH handshake doesn't hang indefinitely
    let _ = tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let _ = tcp.set_write_timeout(Some(std::time::Duration::from_secs(10)));

    let mut sess = Session::new().map_err(|e| AppError::Ssh(format!("Session::new: {e}")))?;
    sess.set_timeout(10_000); // 10s timeout for SSH-level operations
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| AppError::Ssh(format!("SSH handshake with {ip}: {e}")))?;

    sess.userauth_pubkey_file("root", None, private_key_path, None)
        .map_err(|e| AppError::Ssh(format!("SSH auth to {ip}: {e}")))?;

    Ok(sess)
}

/// Execute a command on the remote host and return stdout.
pub fn exec(ip: &str, private_key_path: &Path, command: &str) -> Result<String, AppError> {
    let sess = connect(ip, private_key_path)?;
    let mut channel = sess
        .channel_session()
        .map_err(|e| AppError::Ssh(format!("Open channel: {e}")))?;

    channel
        .exec(command)
        .map_err(|e| AppError::Ssh(format!("Exec command: {e}")))?;

    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|e| AppError::Ssh(format!("Read output: {e}")))?;

    channel
        .wait_close()
        .map_err(|e| AppError::Ssh(format!("Wait close: {e}")))?;

    let exit_status = channel.exit_status().unwrap_or(-1);
    if exit_status != 0 {
        let mut stderr_out = String::new();
        let _ = channel.stderr().read_to_string(&mut stderr_out);
        return Err(AppError::Ssh(format!(
            "Command exited with status {exit_status}: {stderr_out}"
        )));
    }

    Ok(output)
}

/// Upload a local file to the remote host via SCP.
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
    use std::io::Write;
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

/// Download a file from the remote host via SCP.
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
        let result =
            tokio::task::spawn_blocking(move || exec(&ip_clone, &key_clone, "echo ok")).await;

        match result {
            Ok(Ok(_)) => return Ok(()),
            _ => {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

/// Wait for the cloud-init sentinel file to appear on the remote host.
pub async fn wait_for_cloud_init(
    ip: &str,
    private_key_path: &Path,
    timeout: std::time::Duration,
) -> Result<(), AppError> {
    let start = std::time::Instant::now();
    let sentinel = config::CLOUD_INIT_SENTINEL;
    let key = private_key_path.to_path_buf();
    let ip = ip.to_string();
    loop {
        if start.elapsed() > timeout {
            return Err(AppError::Timeout("cloud-init to complete".into()));
        }
        let ip_clone = ip.clone();
        let key_clone = key.clone();
        let cmd = format!("test -f {sentinel} && echo done");
        let result = tokio::task::spawn_blocking(move || exec(&ip_clone, &key_clone, &cmd)).await;

        match result {
            Ok(Ok(out)) if out.trim() == "done" => return Ok(()),
            _ => {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        }
    }
}
