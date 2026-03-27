use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_multi_async;
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

fn ssh_user_for_provider(provider: &Option<String>) -> &'static str {
    match provider.as_deref() {
        Some("lightsail") => "ubuntu",
        _ => "root",
    }
}

pub async fn run(query: &str, plugin: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Installing plugin '{plugin}' on {ip}...");

    // Extract the short plugin name (e.g. @openguardrails/moltguard -> moltguard)
    let short_name = plugin
        .rsplit('/')
        .next()
        .unwrap_or(plugin)
        .trim_start_matches('@');

    let install_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         cd {home}/.openclaw && \
         pnpm add {plugin} 2>&1"
    );
    let enable_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         (openclaw plugins install {plugin} 2>&1 || openclaw plugins enable {short_name} 2>&1 || true)"
    );
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

    println!("[1/3] Installing plugin package...");
    println!("[2/3] Enabling plugin...");
    println!("[3/3] Restarting gateway...");

    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![install_cmd, enable_cmd, restart_cmd.to_string()],
        ssh_user,
    )
    .await?;

    // outputs[0] = install
    if !outputs[0].trim().is_empty() {
        for line in outputs[0].trim().lines().take(5) {
            println!("  {line}");
        }
    }
    // outputs[1] = enable
    if !outputs[1].trim().is_empty() {
        println!("  {}", outputs[1].trim());
    }
    // outputs[2] = restart
    println!("  {}", outputs[2].trim());

    println!("\nPlugin '{plugin}' installed and gateway restarted on {ip}.");
    Ok(())
}
