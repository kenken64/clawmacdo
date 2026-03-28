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
/// SSHs in, resets pairing state, updates .env + gateway.env, enables the plugin,
/// and restarts the gateway. The gateway reads the token from gateway.env automatically —
/// no interactive `channels login` step is needed.
pub async fn configure_bot(query: &str, bot_token: &str, reset: bool) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Configuring Telegram bot on {ip}...");

    // Build command list — all steps share one SSH session.
    let mut cmds: Vec<String> = Vec::new();
    let mut step = 0;
    let total_steps = if reset { 4 } else { 3 };

    if reset {
        // Wipe all pairing state (including allowFrom so the bot forces a fresh pairing flow)
        // and update offsets from any previous bot so the new bot starts with a clean slate.
        step += 1;
        println!("[{step}/{total_steps}] Resetting pairing state...");
        cmds.push(format!(
            "rm -f {home}/.openclaw/credentials/telegram-pairing.json && \
             rm -f {home}/.openclaw/credentials/telegram-default-allowFrom.json && \
             rm -f {home}/.openclaw/telegram/update-offset-*.json && \
             echo 'pairing state cleared'",
        ));
    }

    // Update BOTH .env and gateway.env — the systemd service loads gateway.env via
    // EnvironmentFile, so only updating .env would leave the running service with the old token.
    step += 1;
    println!("[{step}/{total_steps}] Setting TELEGRAM_BOT_TOKEN in .env and gateway.env...");
    cmds.push(format!(
        "for f in {home}/.openclaw/.env {home}/.openclaw/gateway.env; do \
           if [ -f \"$f\" ]; then \
             if grep -q '^TELEGRAM_BOT_TOKEN=' \"$f\" 2>/dev/null; then \
               sed -i 's|^TELEGRAM_BOT_TOKEN=.*|TELEGRAM_BOT_TOKEN={bot_token}|' \"$f\"; \
             else \
               echo 'TELEGRAM_BOT_TOKEN={bot_token}' >> \"$f\"; \
             fi && chmod 600 \"$f\"; \
           fi; \
         done",
    ));

    // Telegram is a stock bundled plugin — just enable it directly, no install needed.
    step += 1;
    println!("[{step}/{total_steps}] Enabling Telegram plugin...");
    cmds.push(format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         (openclaw plugins enable telegram 2>&1 || true)",
    ));

    // Restart the gateway so it picks up the new token from gateway.env.
    step += 1;
    println!("[{step}/{total_steps}] Restarting gateway service...");
    cmds.push(
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)"
            .to_string(),
    );

    let outputs = ssh_as_openclaw_with_user_multi_async(&ip, &key, cmds, ssh_user).await?;

    // Print relevant output
    for (i, out) in outputs.iter().enumerate() {
        let trimmed = out.trim();
        if !trimmed.is_empty() && trimmed != "pairing state cleared" || (reset && i == 0) {
            println!("  {trimmed}");
        }
    }

    println!("\nTelegram bot configured. Send /start to your bot to receive a pairing code.");
    println!("Then run: clawmacdo telegram-pair --instance {query} --code <PAIRING_CODE>");

    Ok(())
}

/// Retrieve the Telegram chat ID from a deployed instance.
/// Searches the openclaw credentials directory for the paired Telegram chat ID.
pub async fn get_chat_id(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Looking up Telegram chat ID on {ip}...");

    // Search for Telegram chat ID in openclaw credentials and data directories
    let cmd = format!(
        "export HOME=\"{home}\" && \
         found=0; \
         for f in {home}/.openclaw/credentials/telegram*.json \
                  {home}/.openclaw/data/telegram*.json \
                  {home}/.openclaw/channels/telegram*.json; do \
           if [ -f \"$f\" ]; then \
             echo \"--- $f ---\"; \
             cat \"$f\"; \
             echo; \
             found=1; \
           fi; \
         done; \
         if [ \"$found\" = 0 ]; then \
           echo 'No Telegram credential files found. Searching .openclaw for chat_id references...'; \
           grep -r -l 'chat.id\\|chatId\\|chat_id\\|telegram' {home}/.openclaw/ 2>/dev/null | head -10 | while read -r match; do \
             echo \"--- $match ---\"; \
             cat \"$match\" 2>/dev/null; \
             echo; \
           done; \
         fi",
    );

    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    if output.trim().is_empty() {
        println!(
            "No Telegram chat ID found. Make sure Telegram is set up and paired on this instance."
        );
    } else {
        println!("{}", output.trim());
    }

    Ok(())
}

/// Reset the Telegram pairing state on a deployed instance.
/// Clears allowFrom, pairing credentials, and update offsets, then restarts the gateway.
/// After reset, send /start to the bot to get a fresh pairing code.
pub async fn reset(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Resetting Telegram pairing on {ip}...");

    let reset_cmd = format!(
        "rm -f {home}/.openclaw/credentials/telegram-pairing.json && \
         rm -f {home}/.openclaw/credentials/telegram-default-allowFrom.json && \
         rm -f {home}/.openclaw/telegram/update-offset-*.json && \
         echo 'pairing state cleared'"
    );
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)";

    println!("[1/2] Clearing pairing credentials...");
    println!("[2/2] Restarting gateway...");
    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![reset_cmd, restart_cmd.to_string()],
        ssh_user,
    )
    .await?;

    println!("  {}", outputs[0].trim());
    println!("  {}", outputs[1].trim());

    println!("\nTelegram pairing reset. Send /start to your bot to receive a new pairing code.");
    println!("Then run: clawmacdo telegram-pair --instance {query} --code <PAIRING_CODE>");

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
