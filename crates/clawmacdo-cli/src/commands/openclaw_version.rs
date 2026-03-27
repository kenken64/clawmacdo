use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use std::path::PathBuf;

/// Fetch available openclaw versions from the npm registry.
/// Returns them as a JSON array of version strings (newest last).
pub async fn list_versions() -> Result<Vec<String>> {
    let output = tokio::process::Command::new("npm")
        .args(["view", "openclaw", "versions", "--json"])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to run npm: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("npm view failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let versions: Vec<String> = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("Failed to parse npm output: {e}"))?;

    Ok(versions)
}

/// CLI handler: print available openclaw versions.
pub async fn run_list(json: bool) -> Result<()> {
    let versions = list_versions().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&versions)?);
    } else {
        // Show last 20 versions (newest last), highlight latest
        let total = versions.len();
        let start = total.saturating_sub(20);
        if start > 0 {
            println!("({start} older versions omitted, use --json for full list)\n");
        }
        for (i, v) in versions[start..].iter().enumerate() {
            if start + i == total - 1 {
                println!("  {v}  (latest)");
            } else {
                println!("  {v}");
            }
        }
        println!("\n{total} versions available.");
    }

    Ok(())
}

/// Look up a deploy record by hostname, IP, or deploy ID.
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

/// CLI handler: install a specific openclaw version on a running instance.
pub async fn run_install(query: &str, version: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Installing openclaw@{version} on {ip}...");

    // Install the specified version
    let install_cmd = format!(
        "export PNPM_HOME={home}/.local/share/pnpm \
         PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} && \
         pnpm install -g openclaw@{version} 2>&1"
    );

    println!("[1/3] Installing openclaw@{version}...");
    let install_out = ssh_as_openclaw_with_user_async(&ip, &key, &install_cmd, ssh_user).await?;
    if !install_out.trim().is_empty() {
        for line in install_out.trim().lines().take(5) {
            println!("  {line}");
        }
    }

    // Verify
    println!("[2/3] Verifying installation...");
    let verify_cmd = format!(
        "PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         openclaw --version"
    );
    let ver_out = ssh_as_openclaw_with_user_async(&ip, &key, &verify_cmd, ssh_user).await?;
    println!("  OpenClaw version: {}", ver_out.trim());

    // Restart gateway
    println!("[3/3] Restarting gateway...");
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
    let restart_out = ssh_as_openclaw_with_user_async(&ip, &key, restart_cmd, ssh_user).await?;
    println!("  {}", restart_out.trim());

    println!("\nopenclaw@{version} installed on {ip}.");

    Ok(())
}
