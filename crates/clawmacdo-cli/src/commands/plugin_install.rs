use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_async;
use std::path::PathBuf;

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

pub async fn run(query: &str, plugin: &str) -> Result<()> {
    let (ip, key, _provider) = find_deploy_record(query)?;
    let home = config::OPENCLAW_HOME;

    println!("Installing plugin '{plugin}' on {ip}...");

    // Step 1: Install the plugin package via pnpm
    println!("[1/3] Installing plugin package...");
    let install_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         cd {home}/.openclaw && \
         pnpm add {plugin} 2>&1"
    );
    let install_out = ssh_as_openclaw_async(&ip, &key, &install_cmd).await?;
    if !install_out.trim().is_empty() {
        println!("  {}", install_out.trim());
    }

    // Step 2: Enable the plugin in openclaw
    println!("[2/3] Enabling plugin...");
    // Extract the short plugin name (e.g. @openguardrails/moltguard -> moltguard)
    let short_name = plugin
        .rsplit('/')
        .next()
        .unwrap_or(plugin)
        .trim_start_matches('@');
    let enable_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         (openclaw plugins enable {short_name} 2>&1 || true)"
    );
    let enable_out = ssh_as_openclaw_async(&ip, &key, &enable_cmd).await?;
    if !enable_out.trim().is_empty() {
        println!("  {}", enable_out.trim());
    }

    // Step 3: Restart gateway
    println!("[3/3] Restarting gateway service...");
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)";
    let restart_out = ssh_as_openclaw_async(&ip, &key, restart_cmd).await?;
    println!("  {}", restart_out.trim());

    println!("\nPlugin '{plugin}' installed and gateway restarted on {ip}.");
    Ok(())
}
