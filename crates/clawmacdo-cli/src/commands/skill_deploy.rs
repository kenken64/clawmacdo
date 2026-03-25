use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_multi_async;
use std::path::{Path, PathBuf};

// Gateway auto-discovers workspace skills from the skills/ subdirectory of the workspace.
const OPENCLAW_WORKSPACE: &str = "/home/openclaw/.openclaw/workspace/skills";

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

/// Build the shell command that extracts a ZIP (handling Windows backslash paths)
/// and fixes permissions. The Python script is base64-encoded and piped to
/// `python3` so it works even when bash's stdin is already in use.
fn build_extract_cmd(zip_path: &str, workspace: &str) -> String {
    let py = format!(
        r#"import zipfile, os
z = zipfile.ZipFile('{zip_path}')
for m in z.namelist():
    safe = m.replace('\\', '/')
    dest = os.path.join('{workspace}', safe)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with z.open(m) as src, open(dest, 'wb') as dst:
        dst.write(src.read())
z.close()
print('extracted OK')
"#
    );
    let b64 = B64.encode(py.as_bytes());
    format!(
        "(echo {b64} | base64 -d | python3 2>&1) && \
         (chown -R openclaw:openclaw {workspace} 2>&1) && \
         (find {workspace} -type f -name '*.md' -exec chmod 644 {{}} \\; 2>&1) && \
         (find {workspace} -type d -exec chmod 755 {{}} \\; 2>&1) && \
         (rm -f {zip_path} 2>/dev/null; true) && \
         echo 'permissions OK'"
    )
}

/// Deploy a ZIP of OpenClaw skills to an instance.
///
/// Steps:
/// 1. Validate and read the local ZIP file
/// 2. SCP the ZIP to /tmp on the instance
/// 3. Extract into ~/.openclaw/workspace/ (preserving directory structure)
/// 4. Fix ownership/permissions
/// 5. Restart the OpenClaw gateway
pub async fn deploy(query: &str, zip_path: &Path) -> Result<()> {
    // Validate file exists and is a zip
    if !zip_path.exists() {
        bail!("File not found: {}", zip_path.display());
    }
    let ext = zip_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "zip" {
        bail!("File must be a .zip archive: {}", zip_path.display());
    }

    let zip_bytes = std::fs::read(zip_path)?;
    if zip_bytes.len() < 4 || &zip_bytes[..4] != b"PK\x03\x04" {
        bail!("Not a valid ZIP file: {}", zip_path.display());
    }

    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!(
        "Deploying skills from {} ({} bytes) to {ip}...",
        zip_path.display(),
        zip_bytes.len()
    );

    // Upload zip to /tmp — accessible for reading by all users.
    // Cleanup uses `|| true` because /tmp has sticky-bit: only the file owner (root/ubuntu)
    // can delete it, but the extract runs as openclaw.
    let tmp_zip = "/tmp/openclaw-skills-upload.zip";
    println!("[1/4] Uploading ZIP to instance...");
    let scp_ip = ip.clone();
    let scp_key = key.clone();
    let scp_bytes = zip_bytes;
    let scp_user = ssh_user.to_string();
    tokio::task::spawn_blocking(move || {
        clawmacdo_ssh::scp_upload_bytes(&scp_ip, &scp_key, &scp_bytes, tmp_zip, 0o644, &scp_user)
    })
    .await??;

    // SSH steps: extract, fix perms, restart — single session
    let extract_cmd = build_extract_cmd(tmp_zip, OPENCLAW_WORKSPACE);
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u openclaw) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u openclaw)/bus && \
         (systemctl --user -M openclaw@ daemon-reload 2>/dev/null || \
          runuser -l openclaw -c 'export XDG_RUNTIME_DIR=/run/user/$(id -u) && systemctl --user daemon-reload' 2>/dev/null || true) && \
         (systemctl --user -M openclaw@ restart openclaw-gateway.service 2>/dev/null || \
          runuser -l openclaw -c 'export XDG_RUNTIME_DIR=/run/user/$(id -u) && systemctl --user restart openclaw-gateway.service' 2>/dev/null || true) && \
         sleep 2 && \
         (runuser -l openclaw -c 'export XDG_RUNTIME_DIR=/run/user/$(id -u) && systemctl --user is-active openclaw-gateway.service' 2>/dev/null || \
          echo 'unknown') ";

    println!("[2/4] Extracting skills to workspace...");
    println!("[3/4] Fixing permissions...");
    println!("[4/4] Restarting gateway...");

    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![extract_cmd, restart_cmd.to_string()],
        ssh_user,
    )
    .await?;

    // outputs[0] = extract + perms
    let extract_out = outputs[0].trim();
    if extract_out.contains("extracted OK") {
        println!("  Skills extracted to workspace.");
    } else {
        println!("  {extract_out}");
    }

    // outputs[1] = restart
    println!("  gateway: {}", outputs[1].trim());

    println!("\nSkills deployed to {ip}.");
    println!("The gateway has been restarted and will pick up the new skills.");

    Ok(())
}
