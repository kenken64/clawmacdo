use anyhow::{bail, Result};
use clawmacdo_core::config;
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

/// Deploy a ZIP of OpenClaw skills to an instance.
///
/// All network I/O (SCP upload + extract + restart) shares a single SSH session,
/// eliminating the extra TCP connect + handshake that two separate sessions would incur.
///
/// Steps:
/// 1. Validate and read the local ZIP file
/// 2. SCP the ZIP to /tmp on the instance  ┐
/// 3. Extract with `unzip` + fix perms      ├── single SSH session
/// 4. Restart the OpenClaw gateway          ┘
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

    let tmp_zip = "/tmp/openclaw-skills-upload.zip";
    let ws = OPENCLAW_WORKSPACE;

    // Extract with `unzip -o` (overwrite, no prompts). Falls back to Python if unzip is missing.
    // Single chmod -R u=rwX,go=rX sets dirs to 755, files to 644 in one pass.
    // chown/chmod use `|| true` because the openclaw user may not be able to chown
    // pre-existing files owned by root from a previous deploy.
    let extract_cmd = format!(
        "mkdir -p {ws} && \
         if command -v unzip >/dev/null 2>&1; then \
           unzip -o {tmp_zip} -d {ws} 2>&1; \
         else \
           python3 -c \"\
import zipfile,os;z=zipfile.ZipFile('{tmp_zip}');\
[os.makedirs(os.path.dirname(os.path.join('{ws}',m.replace(chr(92),'/'))),exist_ok=True) or \
open(os.path.join('{ws}',m.replace(chr(92),'/')),'wb').write(z.read(m)) for m in z.namelist() if not m.endswith('/')];\
z.close();print('extracted OK')\" 2>&1; \
         fi && \
         (chown -R openclaw:openclaw {ws} 2>/dev/null || true) && \
         (chmod -R u=rwX,go=rX {ws} 2>/dev/null || true) && \
         (rm -f {tmp_zip} 2>/dev/null || true) && \
         echo 'extract+perms OK'"
    );

    // Restart gateway; poll up to 3s instead of a fixed sleep 2.
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         for i in 1 2 3; do \
           s=$(systemctl --user is-active openclaw-gateway.service 2>/dev/null) && \
           [ \"$s\" = 'active' ] && break; \
           sleep 1; \
         done && \
         echo \"gateway: $(systemctl --user is-active openclaw-gateway.service 2>/dev/null || echo unknown)\"";

    println!("[1/3] Uploading ZIP to instance...");
    println!("[2/3] Extracting skills + fixing permissions...");
    println!("[3/3] Restarting gateway...");

    // Single SSH session: SCP upload, then run extract + restart commands.
    let scp_ip = ip.clone();
    let scp_key = key.clone();
    let scp_user = ssh_user.to_string();
    let extract_cmd_owned = extract_cmd;
    let restart_cmd_owned = restart_cmd.to_string();
    let outputs = tokio::task::spawn_blocking(move || {
        let cmds: Vec<&str> = vec![&extract_cmd_owned, &restart_cmd_owned];
        clawmacdo_ssh::scp_upload_bytes_and_exec_as(
            &scp_ip, &scp_key, &zip_bytes, tmp_zip, 0o644, &cmds, &scp_user,
        )
    })
    .await??;

    // outputs[0] = extract + perms
    let extract_out = outputs[0].trim();
    if extract_out.contains("extract+perms OK") {
        println!("  Skills extracted to workspace.");
    } else {
        println!("  {extract_out}");
    }

    // outputs[1] = restart
    println!("  {}", outputs[1].trim());

    println!("\nSkills deployed to {ip}.");
    println!("The gateway has been restarted and will pick up the new skills.");

    Ok(())
}
