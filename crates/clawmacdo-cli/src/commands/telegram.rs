use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_async;
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

/// Configure the Telegram bot token on a deployed instance.
/// SSHs in, updates .env, enables the telegram plugin, restarts the gateway,
/// and runs `openclaw channels login --channel telegram` to trigger the pairing message.
pub async fn configure_bot(query: &str, bot_token: &str) -> Result<()> {
    let (ip, key, _provider) = find_deploy_record(query)?;
    let home = config::OPENCLAW_HOME;

    println!("Configuring Telegram bot on {ip}...");

    // Step 1: Update TELEGRAM_BOT_TOKEN in .env
    println!("[1/4] Setting TELEGRAM_BOT_TOKEN in .env...");
    let set_token_cmd = format!(
        "if grep -q '^TELEGRAM_BOT_TOKEN=' {home}/.openclaw/.env 2>/dev/null; then \
           sed -i 's|^TELEGRAM_BOT_TOKEN=.*|TELEGRAM_BOT_TOKEN={bot_token}|' {home}/.openclaw/.env; \
         else \
           echo 'TELEGRAM_BOT_TOKEN={bot_token}' >> {home}/.openclaw/.env; \
         fi && chmod 600 {home}/.openclaw/.env",
    );
    ssh_as_openclaw_async(&ip, &key, &set_token_cmd).await?;

    // Step 2: Enable telegram plugin
    println!("[2/4] Enabling Telegram plugin...");
    let enable_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         (openclaw plugins enable telegram 2>&1 || true)",
    );
    let enable_out = ssh_as_openclaw_async(&ip, &key, &enable_cmd).await?;
    if !enable_out.trim().is_empty() {
        println!("  {}", enable_out.trim());
    }

    // Step 3: Restart gateway
    println!("[3/4] Restarting gateway service...");
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)";
    let restart_out = ssh_as_openclaw_async(&ip, &key, restart_cmd).await?;
    println!("  {}", restart_out.trim());

    // Step 4: Trigger telegram login to get pairing code
    println!("[4/4] Starting Telegram channel login...");
    let login_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if command -v timeout >/dev/null 2>&1; then \
           timeout 20s openclaw channels login --channel telegram 2>&1 || true; \
         else \
           openclaw channels login --channel telegram 2>&1 || true; \
         fi",
    );
    let login_out = ssh_as_openclaw_async(&ip, &key, &login_cmd).await?;
    println!("\n{}", login_out.trim());

    println!("\nTelegram bot configured. Send /start to your bot to receive a pairing code.");
    println!("Then run: clawmacdo telegram pair --instance {query} --code <PAIRING_CODE>");

    Ok(())
}

/// Approve a Telegram pairing code on a deployed instance.
pub async fn approve_pairing(query: &str, code: &str) -> Result<()> {
    let code = code.trim().to_ascii_uppercase();
    if code.len() != 8 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("Invalid pairing code. Must be 8 alphanumeric characters.");
    }

    let (ip, key, _provider) = find_deploy_record(query)?;
    let home = config::OPENCLAW_HOME;

    println!("Approving Telegram pairing code {code} on {ip}...");

    let cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw pairing approve telegram {code} --notify 2>&1",
    );

    let output = ssh_as_openclaw_async(&ip, &key, &cmd).await?;
    println!("{}", output.trim());
    println!("\nTelegram pairing approved. Send a message to your bot to start chatting.");

    Ok(())
}
