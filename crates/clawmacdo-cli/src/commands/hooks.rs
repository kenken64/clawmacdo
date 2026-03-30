use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_with_user_async, ssh_as_openclaw_with_user_multi_async, ssh_root_async,
};
use std::path::PathBuf;

const OPENCLAW_CONFIG: &str = "/home/openclaw/.openclaw/openclaw.json";
const SESSIONS_FILE: &str = "/home/openclaw/.openclaw/agents/main/sessions/sessions.json";

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

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Parse the most-recently-updated recipient for the given channel from
/// sessions.json. Returns `None` when no matching session is found.
fn parse_best_recipient(sessions_json: &str, channel: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(sessions_json.trim()).ok()?;
    let map = root.as_object()?;

    let mut best: Option<(u64, String)> = None;
    for (_key, session) in map {
        let last_channel = session
            .get("lastChannel")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if last_channel != channel {
            continue;
        }
        let updated_at = session
            .get("updatedAt")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let last_to = session.get("lastTo").and_then(|v| v.as_str()).unwrap_or("");

        let recipient = last_to
            .strip_prefix(&format!("{channel}:"))
            .unwrap_or(last_to)
            .to_string();

        if recipient.is_empty() {
            continue;
        }
        if best
            .as_ref()
            .map(|(ts, _)| updated_at > *ts)
            .unwrap_or(true)
        {
            best = Some((updated_at, recipient));
        }
    }

    best.map(|(_, r)| r)
}

/// Extract the funnel https:// URL from `tailscale funnel status` output.
fn parse_funnel_url(status_output: &str) -> Option<String> {
    status_output.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with("https://") {
            let url = trimmed.trim_end_matches(':');
            let url = if let Some(idx) = url.find(" (") {
                &url[..idx]
            } else {
                url
            };
            Some(url.to_string())
        } else {
            None
        }
    })
}

/// Enable webhook hooks on an OpenClaw instance.
///
/// If hooks are already configured and enabled, prints the existing token.
/// Otherwise generates a new token, creates a default "notify" mapping that
/// delivers agent responses to Telegram, and restarts the gateway.
pub async fn enable(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Enabling webhook hooks on {ip}...\n");

    // Session 1: read config + sessions in one SSH connection.
    let read_config_cmd = format!("cat {OPENCLAW_CONFIG} 2>/dev/null || echo '{{}}'");
    let read_sessions_cmd = format!("cat {SESSIONS_FILE} 2>/dev/null || echo '{{}}'");

    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![read_config_cmd, read_sessions_cmd],
        ssh_user,
    )
    .await?;

    let config_json = outputs[0].trim();
    let sessions_json = outputs[1].trim();

    // Check if hooks are already configured.
    let cfg: serde_json::Value =
        serde_json::from_str(config_json).unwrap_or_else(|_| serde_json::json!({}));
    let hooks = cfg.get("hooks");
    if let Some(h) = hooks {
        if h.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false) {
            let token = h
                .get("token")
                .and_then(|v| v.as_str())
                .unwrap_or("(not set)");
            let path = h.get("path").and_then(|v| v.as_str()).unwrap_or("/hooks");
            let mapping_count = h
                .get("mappings")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

            // Get funnel URL for display.
            let funnel_url = match ssh_root_async(&ip, &key, "tailscale funnel status 2>&1").await {
                Ok(out) => parse_funnel_url(&out),
                Err(_) => None,
            };
            let fallback_url = format!("http://{ip}:18789");
            let base_url = funnel_url.as_deref().unwrap_or(&fallback_url);

            println!("Hooks already enabled.");
            println!("  Token:    {token}");
            println!("  Path:     {path}");
            println!("  Mappings: {mapping_count}");
            if let Some(mappings) = h.get("mappings").and_then(|v| v.as_array()) {
                for m in mappings {
                    let mpath = m
                        .get("match")
                        .and_then(|v| v.get("path"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    println!("  Endpoint: {base_url}{path}{mpath}");
                }
            }
            return Ok(());
        }
    }

    // Auto-detect telegram recipient.
    let telegram_to = parse_best_recipient(sessions_json, "telegram");
    if let Some(ref to) = telegram_to {
        println!("[1/3] Auto-detected telegram recipient: {to}");
    } else {
        println!("[1/3] No telegram session found. Notify mapping will not have a --to recipient.");
        println!("       Pair Telegram first, then re-run hooks-enable.");
    }

    // Generate token.
    println!("[2/3] Generating hooks token and default mapping...");
    let home = config::OPENCLAW_HOME;
    let to_json = match &telegram_to {
        Some(to) => format!("\"{to}\""),
        None => "undefined".to_string(),
    };

    let write_cmd = format!(
        r#"export PATH="{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin" && \
         export HOME="{home}" && \
         node -e "
           const fs = require('fs');
           const crypto = require('crypto');
           const cfg = JSON.parse(fs.readFileSync('{OPENCLAW_CONFIG}', 'utf8'));
           const token = crypto.randomBytes(24).toString('hex');
           const to = {to_json};
           const mapping = {{
             id: 'notify',
             match: {{ path: '/notify' }},
             action: 'agent',
             name: 'Webhook Notification',
             sessionKey: 'webhook-notify',
             messageTemplate: '{{{{{{}}}}}}'.replace('{{{{{{}}}}}}', 'task'),
             deliver: true,
             channel: 'telegram',
             allowUnsafeExternalContent: true,
             timeoutSeconds: 120
           }};
           if (to) mapping.to = to;
           cfg.hooks = {{
             enabled: true,
             path: '/hooks',
             token: token,
             defaultSessionKey: 'webhook',
             allowRequestSessionKey: true,
             maxBodyBytes: 1048576,
             mappings: [mapping]
           }};
           fs.writeFileSync('{OPENCLAW_CONFIG}', JSON.stringify(cfg, null, 2) + '\n');
           console.log('TOKEN=' + token);
         ""#
    );

    let restart_cmd = concat!(
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && ",
        "(systemctl --user restart openclaw-gateway.service 2>/dev/null || true) && ",
        "sleep 2 && ",
        "echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)"
    ).to_string();

    // Session 2: write config + restart.
    println!("[3/3] Updating openclaw.json and restarting gateway...");
    let outputs =
        ssh_as_openclaw_with_user_multi_async(&ip, &key, vec![write_cmd, restart_cmd], ssh_user)
            .await?;

    let write_out = outputs[0].trim();
    let restart_out = outputs[1].trim();
    println!("  {restart_out}");

    let token = write_out
        .lines()
        .find_map(|l| l.strip_prefix("TOKEN="))
        .unwrap_or("(unknown)");

    // Get funnel URL for display.
    let funnel_url = match ssh_root_async(&ip, &key, "tailscale funnel status 2>&1").await {
        Ok(out) => parse_funnel_url(&out),
        Err(_) => None,
    };
    let fallback_url2 = format!("http://{ip}:18789");
    let base_url = funnel_url.as_deref().unwrap_or(&fallback_url2);

    println!("\nWebhook hooks enabled!");
    println!("  Token:    {token}");
    println!("  Endpoint: {base_url}/hooks/notify");

    println!("\nUsage:");
    println!("  clawmacdo hooks-send --instance {query} --task \"your instruction\"");
    println!("\nOr via curl:");
    println!("  curl -X POST {base_url}/hooks/notify \\");
    println!("    -H \"Authorization: Bearer {token}\" \\");
    println!("    -H \"Content-Type: application/json\" \\");
    println!("    -d '{{\"task\":\"your instruction to the agent\"}}'");

    Ok(())
}

/// Disable webhook hooks on an OpenClaw instance.
pub async fn disable(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Disabling webhook hooks on {ip}...");

    let patch_cmd = format!(
        r#"export PATH="{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin" && \
         export HOME="{home}" && \
         node -e "
           const fs = require('fs');
           const cfg = JSON.parse(fs.readFileSync('{OPENCLAW_CONFIG}', 'utf8'));
           if (cfg.hooks) cfg.hooks.enabled = false;
           fs.writeFileSync('{OPENCLAW_CONFIG}', JSON.stringify(cfg, null, 2) + '\n');
           console.log('hooks.enabled: false');
         ""#
    );

    let restart_cmd = concat!(
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && ",
        "(systemctl --user restart openclaw-gateway.service 2>/dev/null || true) && ",
        "sleep 2 && ",
        "echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)"
    ).to_string();

    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![patch_cmd, restart_cmd.to_string()],
        ssh_user,
    )
    .await?;

    println!("  {}", outputs[0].trim());
    println!("  {}", outputs[1].trim());
    println!("\nWebhook hooks disabled.");

    Ok(())
}

/// Show webhook hooks status and mappings on an OpenClaw instance.
pub async fn status(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    let cmd = format!("cat {OPENCLAW_CONFIG} 2>/dev/null || echo '{{}}'");
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let cfg: serde_json::Value =
        serde_json::from_str(output.trim()).unwrap_or_else(|_| serde_json::json!({}));

    let hooks = cfg.get("hooks");
    match hooks {
        None => {
            println!("Webhook hooks on {ip}: not configured");
            println!("\nRun `clawmacdo hooks-enable --instance {query}` to set up.");
            return Ok(());
        }
        Some(h) => {
            let enabled = h.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            let token = h
                .get("token")
                .and_then(|v| v.as_str())
                .unwrap_or("(not set)");
            let path = h.get("path").and_then(|v| v.as_str()).unwrap_or("/hooks");

            // Mask token: show first 8 chars.
            let token_display = if token.len() > 8 {
                let prefix = &token[..8];
                format!("{prefix}...")
            } else {
                token.to_string()
            };

            println!("Webhook hooks on {ip}:");
            println!("  Enabled: {}", if enabled { "yes" } else { "no" });
            println!("  Token:   {token_display}");
            println!("  Path:    {path}");

            let mappings = h.get("mappings").and_then(|v| v.as_array());
            match mappings {
                Some(arr) if !arr.is_empty() => {
                    let hdr = format!(
                        "\n  {:<10} {:<10} {:<8} {:<8} {:<10} To",
                        "ID", "Path", "Action", "Deliver", "Channel"
                    );
                    println!("{hdr}");
                    let sep = format!(
                        "  {:<10} {:<10} {:<8} {:<8} {:<10} ----------",
                        "----------", "----------", "--------", "--------", "----------"
                    );
                    println!("{sep}");
                    for m in arr {
                        let mid = m.get("id").and_then(|v| v.as_str()).unwrap_or("-");
                        let mpath = m
                            .get("match")
                            .and_then(|v| v.get("path"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");
                        let action = m.get("action").and_then(|v| v.as_str()).unwrap_or("-");
                        let deliver = m
                            .get("deliver")
                            .and_then(|v| v.as_bool())
                            .map(|b| if b { "yes" } else { "no" })
                            .unwrap_or("-");
                        let channel = m.get("channel").and_then(|v| v.as_str()).unwrap_or("-");
                        let to = m.get("to").and_then(|v| v.as_str()).unwrap_or("-");
                        println!(
                            "  {mid:<10} {mpath:<10} {action:<8} {deliver:<8} {channel:<10} {to}"
                        );
                    }
                }
                _ => {
                    println!("\n  No mappings configured.");
                }
            }
        }
    }

    Ok(())
}

/// Send a task to an OpenClaw instance via webhook hooks.
///
/// Reads the hooks token from the server, then executes a curl POST to
/// `http://127.0.0.1:18789/hooks/<mapping-path>` on the server itself.
/// Uses concurrent SSH calls for config read + funnel URL lookup, then
/// a single SSH call to execute curl — 2 connections total.
pub async fn send(query: &str, task: &str, mapping_id: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Sending task to {ip}...");

    // Concurrent SSH: read config (as openclaw) + funnel status (as root).
    let config_cmd = format!("cat {OPENCLAW_CONFIG} 2>/dev/null || echo '{{}}'");
    let config_fut = ssh_as_openclaw_with_user_async(&ip, &key, &config_cmd, ssh_user);
    let funnel_fut = ssh_root_async(&ip, &key, "tailscale funnel status 2>&1");
    let (config_res, funnel_res) = tokio::join!(config_fut, funnel_fut);

    let config_out = config_res?;
    let funnel_url = funnel_res.ok().and_then(|out| parse_funnel_url(&out));

    let cfg: serde_json::Value =
        serde_json::from_str(config_out.trim()).unwrap_or_else(|_| serde_json::json!({}));

    let hooks = cfg.get("hooks").ok_or_else(|| {
        anyhow::anyhow!(
            "Hooks not configured. Run `clawmacdo hooks-enable --instance {query}` first."
        )
    })?;

    if !hooks
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        bail!("Hooks are disabled. Run `clawmacdo hooks-enable --instance {query}` first.");
    }

    let token = hooks
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("No hooks token found in config."))?;

    let hooks_path = hooks
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("/hooks");

    // Find the mapping.
    let mappings = hooks
        .get("mappings")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("No hook mappings configured."))?;

    let mapping = mappings
        .iter()
        .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(mapping_id))
        .ok_or_else(|| {
            let ids: Vec<&str> = mappings
                .iter()
                .filter_map(|m| m.get("id").and_then(|v| v.as_str()))
                .collect();
            anyhow::anyhow!(
                "No mapping '{mapping_id}' found. Available: {}",
                ids.join(", ")
            )
        })?;

    let match_path = mapping
        .get("match")
        .and_then(|v| v.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("/notify");

    let local_endpoint = format!("http://127.0.0.1:18789{hooks_path}{match_path}");
    let public_endpoint = funnel_url
        .as_deref()
        .map(|u| format!("{u}{hooks_path}{match_path}"));

    println!(
        "  Endpoint: {}",
        public_endpoint.as_deref().unwrap_or(&local_endpoint)
    );
    println!("  Task:     {task}");

    // Build the JSON payload with proper escaping.
    let payload = serde_json::json!({ "task": task });
    let payload_str = serde_json::to_string(&payload)?;

    // Execute curl on the server itself (localhost — avoids funnel issues).
    let curl_cmd = format!(
        "curl -s -X POST '{}' \
         -H 'Authorization: Bearer {}' \
         -H 'Content-Type: application/json' \
         -d {} 2>&1",
        local_endpoint,
        token,
        shell_escape(&payload_str)
    );

    let result = ssh_as_openclaw_with_user_async(&ip, &key, &curl_cmd, ssh_user).await?;
    let result = result.trim();

    println!("\n  {result}");

    // Parse response for user-friendly output.
    if let Ok(resp) = serde_json::from_str::<serde_json::Value>(result) {
        if resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            let run_id = resp
                .get("runId")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let deliver = mapping
                .get("deliver")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let channel = mapping
                .get("channel")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            println!("\nTask sent (runId: {run_id}).");
            if deliver {
                println!("The agent will deliver the response to {channel}.");
            }
        } else {
            let err = resp
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            println!("\nFailed: {err}");
        }
    }

    Ok(())
}
