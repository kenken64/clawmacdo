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
        Some("azure") => "azureuser",
        _ => "root",
    }
}

fn js_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn build_configure_cmd(bot_token: &str, reset: bool) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let token = js_string(bot_token)?;
    let reset = if reset { "true" } else { "false" };

    let mut cmd = r#"export HOME="__OPENCLAW_HOME__" && \
         node <<'NODE'
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '__OPENCLAW_HOME__';
const token = __TOKEN_JSON__;
const reset = __RESET_BOOL__;
const openclawDir = path.join(home, '.openclaw');
const configPath = path.join(openclawDir, 'openclaw.json');
const envFiles = [
  path.join(openclawDir, '.env'),
  path.join(openclawDir, 'gateway.env')
];

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (_) {
    return {};
  }
}

function object(value) {
  return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

function updateEnv(file) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  let lines = [];
  try {
    lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
    if (lines.length && lines[lines.length - 1] === '') lines.pop();
  } catch (_) {}

  let seen = false;
  lines = lines
    .map((line) => {
      if (line.startsWith('TELEGRAM_BOT_TOKEN=')) {
        seen = true;
        return `TELEGRAM_BOT_TOKEN=${token}`;
      }
      return line;
    });
  if (!seen) lines.push(`TELEGRAM_BOT_TOKEN=${token}`);
  fs.writeFileSync(file, lines.join('\n') + '\n', { mode: 0o600 });
  fs.chmodSync(file, 0o600);
}

function enableTelegramConfig() {
  const cfg = readJson(configPath);
  cfg.channels = object(cfg.channels);
  cfg.channels.telegram = object(cfg.channels.telegram);
  cfg.channels.telegram.enabled = true;
  cfg.channels.telegram.botToken = token;
  cfg.channels.telegram.dmPolicy = cfg.channels.telegram.dmPolicy || 'pairing';
  cfg.channels.telegram.groupPolicy = cfg.channels.telegram.groupPolicy || 'allowlist';
  const streaming = cfg.channels.telegram.streaming;
  if (streaming !== undefined && (!streaming || typeof streaming !== 'object' || Array.isArray(streaming))) {
    delete cfg.channels.telegram.streaming;
  }

  fs.mkdirSync(path.dirname(configPath), { recursive: true });
  if (fs.existsSync(configPath)) {
    fs.copyFileSync(configPath, configPath + '.clawmacdo-telegram.bak');
  }
  fs.writeFileSync(configPath, JSON.stringify(cfg, null, 2) + '\n', { mode: 0o600 });
  fs.chmodSync(configPath, 0o600);
}

if (reset) {
  for (const file of [
    path.join(openclawDir, 'credentials', 'telegram-pairing.json'),
    path.join(openclawDir, 'credentials', 'telegram-default-allowFrom.json')
  ]) {
    fs.rmSync(file, { force: true });
  }
  const telegramDir = path.join(openclawDir, 'telegram');
  try {
    for (const name of fs.readdirSync(telegramDir)) {
      if (/^update-offset-.*\.json$/.test(name)) {
        fs.rmSync(path.join(telegramDir, name), { force: true });
      }
    }
  } catch (_) {}
  console.log('reset: ok');
}

for (const file of envFiles) updateEnv(file);
enableTelegramConfig();
console.log('token: ok');
console.log('channel: telegram enabled');
NODE"#
        .to_string();

    for (needle, value) in [
        ("__OPENCLAW_HOME__", home.to_string()),
        ("__TOKEN_JSON__", token),
        ("__RESET_BOOL__", reset.to_string()),
    ] {
        cmd = cmd.replace(needle, &value);
    }

    Ok(cmd)
}

fn restart_gateway_cmd() -> String {
    format!(
        "grep -q '^TELEGRAM_BOT_TOKEN=' {home}/.openclaw/gateway.env 2>/dev/null || \
         {{ echo 'restart: FAILED - token not in gateway.env'; exit 1; }}; \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         for i in $(seq 1 12); do \
           if curl -fsS --max-time 1 http://127.0.0.1:18789/health >/dev/null 2>&1; then echo 'gateway: healthy'; exit 0; fi; \
           STATE=$(systemctl --user is-active openclaw-gateway.service 2>/dev/null || true); \
           if [ \"$STATE\" = \"active\" ] && [ \"$i\" -ge 2 ]; then echo 'gateway: active'; exit 0; fi; \
           sleep 1; \
         done; \
         echo 'gateway: FAILED - service not healthy after restart'; exit 1",
        home = config::OPENCLAW_HOME
    )
}

/// Configure the Telegram bot token on a deployed instance.
/// SSHs in, resets pairing state, updates .env + gateway.env, updates channel config,
/// and restarts the gateway. The gateway reads the token from gateway.env automatically —
/// no interactive `channels login` step is needed.
pub async fn configure_bot(query: &str, bot_token: &str, reset: bool) -> Result<()> {
    let bot_token = bot_token.trim();
    if bot_token.is_empty() || bot_token.chars().any(char::is_control) {
        bail!("Telegram bot token cannot be empty or contain control characters.");
    }

    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Configuring Telegram bot on {ip}...");

    println!("[1/2] Updating token, Telegram channel config, and pairing state...");
    println!("[2/2] Restarting gateway service...");

    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![
            build_configure_cmd(bot_token, reset)?,
            restart_gateway_cmd(),
        ],
        ssh_user,
    )
    .await?;

    for out in &outputs {
        let trimmed = out.trim();
        if !trimmed.is_empty() {
            println!("  {trimmed}");
        }
    }

    println!("\nTelegram bot configured. Send /start to your bot to receive a pairing code.");
    println!("Then run: clawmacdo telegram-pair --instance {query} --code <PAIRING_CODE>");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configure_cmd_removes_legacy_telegram_streaming_string() {
        let cmd = build_configure_cmd("123456:abcdef", false).unwrap();
        assert!(cmd.contains("delete cfg.channels.telegram.streaming"));
        assert!(!cmd.contains("streaming || 'partial'"));
    }
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
         openclaw pairing approve telegram {code} 2>&1 && \
         (openclaw pairing approve telegram {code} --notify >/dev/null 2>&1 &) && \
         echo 'pairing approved'",
    );

    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    println!("{}", output.trim());
    println!("\nTelegram pairing approved. Send a message to your bot to start chatting.");

    Ok(())
}
