use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_async;
use std::path::{Path, PathBuf};

const OPENCLAW_SKILL_PATH: &str = "/home/openclaw/.openclaw/workspace/SKILL.md";

/// Look up a deploy record by hostname, IP, or deploy ID.
/// Returns (id, ip, ssh_key_path).
fn find_deploy_record(query: &str) -> Result<(String, String, PathBuf)> {
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
            return Ok((
                record.id,
                record.ip_address,
                PathBuf::from(record.ssh_key_path),
            ));
        }
    }

    bail!("No deploy record found for '{query}'. Use a deploy ID, hostname, or IP address.");
}

/// Upload a local SKILL.md to the Railway skills API and SCP it to the instance.
///
/// Steps:
/// 1. Read the local SKILL.md file
/// 2. Upload to Railway API: POST /api/user-skills/<deployment-id>
/// 3. Backup existing SKILL.md on the OpenClaw instance
/// 4. SCP the SKILL.md to the instance workspace
pub async fn upload(query: &str, skill_file: &Path, api_url: &str, api_key: &str) -> Result<()> {
    let (deploy_id, ip, ssh_key) = find_deploy_record(query)?;

    // Step 1: Read local file
    if !skill_file.exists() {
        bail!("SKILL.md not found at: {}", skill_file.display());
    }
    let file_bytes = std::fs::read(skill_file)?;
    println!(
        "Uploading SKILL.md ({} bytes) for deployment {deploy_id}...\n",
        file_bytes.len()
    );

    // Step 2: Upload to Railway API
    println!("[1/3] Uploading to skills API...");
    let upload_url = format!(
        "{}/api/user-skills/{}",
        api_url.trim_end_matches('/'),
        deploy_id
    );
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(file_bytes.clone())
            .file_name("SKILL.md")
            .mime_str("text/markdown")?,
    );

    let resp = client
        .post(&upload_url)
        .header("x-api-key", api_key)
        .multipart(form)
        .send()
        .await?;

    let status = resp.status();
    let body: serde_json::Value = resp.json().await?;

    if status.is_success() {
        let backed_up = body
            .get("backed_up")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        println!("  Uploaded to Railway volume.");
        if backed_up {
            println!("  Previous version backed up on server.");
        }
    } else {
        let err_msg = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        bail!("API upload failed ({status}): {err_msg}");
    }

    // Step 3: Backup existing SKILL.md on the instance
    println!("[2/3] Backing up existing SKILL.md on instance {ip}...");
    let backup_cmd = format!(
        "if [ -f {OPENCLAW_SKILL_PATH} ]; then \
           cp {OPENCLAW_SKILL_PATH} {OPENCLAW_SKILL_PATH}.backup-$(date +%Y%m%dT%H%M%S) && \
           echo 'BACKED_UP'; \
         else \
           echo 'NO_EXISTING'; \
         fi"
    );
    let backup_out = ssh_as_openclaw_async(&ip, &ssh_key, &backup_cmd).await?;
    let backup_trimmed = backup_out.trim();
    if backup_trimmed.contains("BACKED_UP") {
        println!("  Existing SKILL.md backed up.");
    } else {
        println!("  No existing SKILL.md to back up.");
    }

    // Step 4: SCP the file to the instance
    println!("[3/3] Uploading SKILL.md to instance via SCP...");
    // Write to /tmp first, then move as openclaw user
    let tmp_path = "/tmp/SKILL.md.upload";
    let scp_ip = ip.clone();
    let scp_key = ssh_key.clone();
    let scp_bytes = file_bytes;
    tokio::task::spawn_blocking(move || {
        clawmacdo_ssh::scp_upload_bytes(&scp_ip, &scp_key, &scp_bytes, tmp_path, 0o644, "root")
    })
    .await??;

    let move_cmd = format!(
        "cp {tmp_path} {OPENCLAW_SKILL_PATH} && chmod 644 {OPENCLAW_SKILL_PATH} && rm -f {tmp_path} && echo 'OK'"
    );
    let move_out = ssh_as_openclaw_async(&ip, &ssh_key, &move_cmd).await?;
    if move_out.trim().contains("OK") {
        println!("  SKILL.md deployed to instance.");
    } else {
        println!("  Warning: {}", move_out.trim());
    }

    println!("\nDone! SKILL.md uploaded to both Railway and instance {ip}.");
    Ok(())
}

/// Download a SKILL.md from the Railway skills API.
///
/// Steps:
/// 1. Download from Railway API: GET /api/user-skills/<deployment-id>
/// 2. Save to local file
pub async fn download(query: &str, output_path: &Path, api_url: &str, api_key: &str) -> Result<()> {
    let (deploy_id, _ip, _ssh_key) = find_deploy_record(query)?;

    println!("Downloading SKILL.md for deployment {deploy_id}...\n");

    let download_url = format!(
        "{}/api/user-skills/{}",
        api_url.trim_end_matches('/'),
        deploy_id
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&download_url)
        .header("x-api-key", api_key)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let err_msg = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        bail!("API download failed ({status}): {err_msg}");
    }

    let bytes = resp.bytes().await?;
    std::fs::write(output_path, &bytes)?;
    println!(
        "Downloaded SKILL.md ({} bytes) to {}",
        bytes.len(),
        output_path.display()
    );

    Ok(())
}

/// Push an existing SKILL.md from the Railway API directly to the OpenClaw instance.
///
/// Steps:
/// 1. Download from Railway API
/// 2. Backup existing on instance
/// 3. SCP to instance
pub async fn push_to_instance(query: &str, api_url: &str, api_key: &str) -> Result<()> {
    let (deploy_id, ip, ssh_key) = find_deploy_record(query)?;

    println!("Pushing SKILL.md from Railway to instance {ip}...\n");

    // Step 1: Download from API
    println!("[1/3] Downloading from skills API...");
    let download_url = format!(
        "{}/api/user-skills/{}",
        api_url.trim_end_matches('/'),
        deploy_id
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&download_url)
        .header("x-api-key", api_key)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let err_msg = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        bail!("API download failed ({status}): {err_msg}");
    }

    let file_bytes = resp.bytes().await?.to_vec();
    println!("  Downloaded {} bytes.", file_bytes.len());

    // Step 2: Backup existing on instance
    println!("[2/3] Backing up existing SKILL.md on instance...");
    let backup_cmd = format!(
        "if [ -f {OPENCLAW_SKILL_PATH} ]; then \
           cp {OPENCLAW_SKILL_PATH} {OPENCLAW_SKILL_PATH}.backup-$(date +%Y%m%dT%H%M%S) && \
           echo 'BACKED_UP'; \
         else \
           echo 'NO_EXISTING'; \
         fi"
    );
    let backup_out = ssh_as_openclaw_async(&ip, &ssh_key, &backup_cmd).await?;
    if backup_out.trim().contains("BACKED_UP") {
        println!("  Existing SKILL.md backed up.");
    } else {
        println!("  No existing SKILL.md to back up.");
    }

    // Step 3: SCP to instance
    println!("[3/3] Uploading to instance via SCP...");
    let tmp_path = "/tmp/SKILL.md.upload";
    let scp_ip = ip.clone();
    let scp_key = ssh_key.clone();
    let scp_bytes = file_bytes;
    tokio::task::spawn_blocking(move || {
        clawmacdo_ssh::scp_upload_bytes(&scp_ip, &scp_key, &scp_bytes, tmp_path, 0o644, "root")
    })
    .await??;

    let move_cmd = format!(
        "cp {tmp_path} {OPENCLAW_SKILL_PATH} && chmod 644 {OPENCLAW_SKILL_PATH} && rm -f {tmp_path} && echo 'OK'"
    );
    let move_out = ssh_as_openclaw_async(&ip, &ssh_key, &move_cmd).await?;
    if move_out.trim().contains("OK") {
        println!("  SKILL.md deployed to instance.");
    } else {
        println!("  Warning: {}", move_out.trim());
    }

    println!("\nDone! SKILL.md pushed to instance {ip}.");
    Ok(())
}
