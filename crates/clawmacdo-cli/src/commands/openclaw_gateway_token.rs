use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_multi_async;
use std::path::PathBuf;

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
        Some("azure") => "azureuser",
        _ => "root",
    }
}

fn rotate_token_cmd() -> String {
    let home = config::OPENCLAW_HOME;
    format!(
        r#"export HOME="{home}" && \
         CONFIG="{home}/.openclaw/openclaw.json" && \
         node <<'NODE'
const fs = require('fs');
const crypto = require('crypto');

const configPath = process.env.CONFIG || '{home}/.openclaw/openclaw.json';
if (!fs.existsSync(configPath)) {{
  console.error(`openclaw.json not found at ${{configPath}}`);
  process.exit(1);
}}

const cfg = JSON.parse(fs.readFileSync(configPath, 'utf8'));
const backupPath = configPath + '.bak';
fs.copyFileSync(configPath, backupPath);

cfg.gateway = cfg.gateway || {{}};
cfg.gateway.auth = cfg.gateway.auth || {{}};

const oldToken = cfg.gateway.auth.token || '';
const newToken = crypto.randomBytes(32).toString('hex');
cfg.gateway.auth.token = newToken;

// Funnel/password auth mirrors the gateway token into password. Keep it in sync
// without forcing password auth on instances that are using a different mode.
if (cfg.gateway.auth.mode === 'password' || typeof cfg.gateway.auth.password === 'string') {{
  cfg.gateway.auth.password = newToken;
}}

fs.writeFileSync(configPath, JSON.stringify(cfg, null, 2) + '\n', {{ mode: 0o600 }});

console.log('TOKEN=' + newToken);
console.log('OLD_TOKEN_PREFIX=' + (oldToken ? oldToken.slice(0, 8) : ''));
console.log('BACKUP=' + backupPath);
NODE"#,
        home = home
    )
}

fn restart_cmd() -> String {
    "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
     (systemctl --user daemon-reload 2>/dev/null || true) && \
     (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
      systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
     sleep 2 && \
     echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)"
        .to_string()
}

pub async fn run(query: &str) -> Result<()> {
    let query = query.trim();
    if query.is_empty() {
        bail!("--instance cannot be empty.");
    }

    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Regenerating OpenClaw gateway token on {ip}...");
    println!("[1/2] Updating openclaw.json...");
    println!("[2/2] Restarting gateway...");

    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![rotate_token_cmd(), restart_cmd()],
        ssh_user,
    )
    .await?;

    let rotate_out = outputs[0].trim();
    let restart_out = outputs[1].trim();

    let token = rotate_out
        .lines()
        .find_map(|line| line.strip_prefix("TOKEN="))
        .unwrap_or("");
    let old_prefix = rotate_out
        .lines()
        .find_map(|line| line.strip_prefix("OLD_TOKEN_PREFIX="))
        .unwrap_or("");
    let backup = rotate_out
        .lines()
        .find_map(|line| line.strip_prefix("BACKUP="))
        .unwrap_or("");

    println!("  {restart_out}");
    println!();
    println!("OpenClaw gateway token regenerated on {ip}:");
    if !old_prefix.is_empty() {
        println!("  Previous token prefix: {old_prefix}...");
    }
    if !backup.is_empty() {
        println!("  Backup: {backup}");
    }
    println!("  New gateway token: {token}");

    Ok(())
}
