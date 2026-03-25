use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_with_user_async, ssh_as_openclaw_with_user_multi_async,
};
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
        _ => "root",
    }
}

/// Configure the Telegram bot token on a deployed instance.
/// SSHs in, updates .env, enables the telegram plugin, restarts the gateway,
/// and runs `openclaw channels login --channel telegram` to trigger the pairing message.
pub async fn configure_bot(query: &str, bot_token: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Configuring Telegram bot on {ip}...");

    // Wipe any pairing credentials and Telegram update offsets from a previous bot so
    // the new bot starts with a clean slate (old pairing requests are useless for the new token).
    let reset_cmd = format!(
        "rm -f {home}/.openclaw/credentials/telegram-pairing.json && \
         rm -f {home}/.openclaw/telegram/update-offset-*.json && \
         echo 'pairing state cleared'",
    );
    // Update BOTH .env and gateway.env — the systemd service loads gateway.env via
    // EnvironmentFile, so only updating .env would leave the running service with the old token.
    let set_token_cmd = format!(
        "for f in {home}/.openclaw/.env {home}/.openclaw/gateway.env; do \
           if [ -f \"$f\" ]; then \
             if grep -q '^TELEGRAM_BOT_TOKEN=' \"$f\" 2>/dev/null; then \
               sed -i 's|^TELEGRAM_BOT_TOKEN=.*|TELEGRAM_BOT_TOKEN={bot_token}|' \"$f\"; \
             else \
               echo 'TELEGRAM_BOT_TOKEN={bot_token}' >> \"$f\"; \
             fi && chmod 600 \"$f\"; \
           fi; \
         done",
    );
    // Install the Telegram plugin from npm non-interactively.
    // `openclaw plugins install` launches an interactive TUI that cannot complete over SSH;
    // install the npm package directly via pnpm (openclaw's bundled package manager) instead,
    // then enable it. Fall back to the interactive install (piping option 2 = npm registry)
    // if pnpm is unavailable.
    let install_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         if ! openclaw plugins list 2>/dev/null | grep -qi 'telegram'; then \
           pnpm add --dir {home}/.openclaw @openclaw/telegram 2>&1 || \
           npm install --prefix {home}/.openclaw @openclaw/telegram 2>&1 || \
           printf '2\\n' | timeout 30s openclaw plugins install @openclaw/telegram 2>&1 || true; \
         fi && \
         (openclaw plugins enable telegram 2>&1 || true)",
    );
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)";
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

    // All 5 steps share one SSH session — one TCP connect + handshake.
    println!("[1/5] Resetting pairing state for new bot...");
    println!("[2/5] Setting TELEGRAM_BOT_TOKEN in .env and gateway.env...");
    println!("[3/5] Installing/enabling Telegram plugin...");
    println!("[4/5] Restarting gateway service...");
    println!("[5/5] Starting Telegram channel login...");
    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![
            reset_cmd,
            set_token_cmd,
            install_cmd,
            restart_cmd.to_string(),
            login_cmd,
        ],
        ssh_user,
    )
    .await?;

    // outputs[0] = reset (print confirmation)
    println!("  {}", outputs[0].trim());
    // outputs[1] = set_token (discard)
    // outputs[2] = install/enable plugin
    if !outputs[2].trim().is_empty() {
        println!("  {}", outputs[2].trim());
    }
    // outputs[3] = restart gateway
    println!("  {}", outputs[3].trim());
    // outputs[4] = login
    println!("\n{}", outputs[4].trim());

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

    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Approving Telegram pairing code {code} on {ip}...");

    let cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw pairing approve telegram {code} --notify 2>&1",
    );

    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    println!("{}", output.trim());
    println!("\nTelegram pairing approved. Send a message to your bot to start chatting.");

    Ok(())
}
