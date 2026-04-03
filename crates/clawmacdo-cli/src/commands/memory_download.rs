use anyhow::{bail, Result};
use chrono::Utc;
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_root_as_async;
use std::path::{Path, PathBuf};

const REMOTE_MEMORY_DIR: &str = "/home/openclaw/.openclaw/memory";

/// Look up a deploy record by hostname, IP, or deploy ID.
/// Returns (ip, ssh_key_path, provider).
fn find_deploy_record(query: &str) -> Result<(String, PathBuf, Option<String>)> {
    let deploys_dir = config::deploys_dir()?;
    if !deploys_dir.exists() {
        bail!("No deploy records found. Deploy an instance first.");
    }

    for entry in std::fs::read_dir(&deploys_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let contents = std::fs::read_to_string(&path)?;
        let record: config::DeployRecord = match serde_json::from_str(&contents) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if record.id == query || record.hostname == query || record.ip_address == query {
            let provider = record.provider.map(|p| p.to_string());
            return Ok((
                record.ip_address,
                PathBuf::from(record.ssh_key_path),
                provider,
            ));
        }
    }

    bail!("No deploy record found for '{query}'. Use a deploy ID, hostname, or IP address.");
}

fn ssh_user_for_provider(provider: &Option<String>) -> &'static str {
    match provider.as_deref() {
        Some("lightsail") => "ubuntu",
        _ => "root",
    }
}

/// Download all memory archive files from an OpenClaw instance.
///
/// Steps:
/// 1. SSH into the instance and verify memory directory exists
/// 2. Create a tar.gz archive of memory files on the remote host
/// 3. SCP download the archive to the local output path
/// 4. Clean up the temporary archive on the remote host
pub async fn run(query: &str, output: &Path) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Connecting to {ip}...");

    // Check if memory directory exists and list its contents
    let check_cmd = format!(
        "if [ -d {REMOTE_MEMORY_DIR} ]; then \
           count=$(find {REMOTE_MEMORY_DIR} -type f 2>/dev/null | wc -l | tr -d ' '); \
           echo \"EXISTS:$count\"; \
         else \
           echo 'NOT_FOUND'; \
         fi"
    );
    let check_output = ssh_root_as_async(&ip, &key, &check_cmd, ssh_user).await?;
    let check_output = check_output.trim();

    if check_output == "NOT_FOUND" {
        bail!(
            "Memory directory not found on instance {ip}.\nExpected: {REMOTE_MEMORY_DIR}\n\
             The instance may not have any memory archives yet."
        );
    }

    let file_count: u64 = check_output
        .strip_prefix("EXISTS:")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if file_count == 0 {
        bail!("Memory directory exists but contains no files on instance {ip}.");
    }

    println!("  Found {file_count} memory archive file(s).");

    // Create tar.gz on the remote host
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let remote_archive = format!("/tmp/openclaw-memory-{timestamp}.tar.gz");

    println!("  Creating archive on instance...");
    let tar_cmd = format!(
        "tar czf {remote_archive} -C /home/openclaw/.openclaw memory 2>&1 && echo 'TAR_OK'"
    );
    let tar_output = ssh_root_as_async(&ip, &key, &tar_cmd, ssh_user).await?;
    if !tar_output.contains("TAR_OK") {
        bail!(
            "Failed to create archive on instance: {}",
            tar_output.trim()
        );
    }

    // Get archive size
    let size_cmd = format!(
        "stat -c '%s' {remote_archive} 2>/dev/null || stat -f '%z' {remote_archive} 2>/dev/null"
    );
    let size_str = ssh_root_as_async(&ip, &key, &size_cmd, ssh_user)
        .await
        .unwrap_or_default();
    let archive_size: u64 = size_str.trim().parse().unwrap_or(0);

    // Determine local output path
    let local_path = if output.is_dir() {
        output.join(format!("openclaw-memory-{timestamp}.tar.gz"))
    } else {
        output.to_path_buf()
    };

    // Ensure parent directory exists
    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // SCP download
    println!("  Downloading archive ({})...", format_size(archive_size));
    let scp_ip = ip.clone();
    let scp_key = key.clone();
    let scp_remote = remote_archive.clone();
    let scp_local = local_path.clone();
    let scp_user = ssh_user.to_string();
    tokio::task::spawn_blocking(move || {
        clawmacdo_ssh::scp_download_as(&scp_ip, &scp_key, &scp_remote, &scp_local, &scp_user)
    })
    .await??;

    // Clean up remote archive
    let cleanup_cmd = format!("rm -f {remote_archive}");
    let _ = ssh_root_as_async(&ip, &key, &cleanup_cmd, ssh_user).await;

    println!("\nMemory archives downloaded to: {}", local_path.display());
    println!("  {} file(s), {}", file_count, format_size(archive_size));

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
