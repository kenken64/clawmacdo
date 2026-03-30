use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_with_user_async, ssh_as_openclaw_with_user_multi_async,
};
use std::path::PathBuf;

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
        _ => "root",
    }
}

/// Auto-approve all pending device pairing requests using the openclaw CLI.
/// The CLI uses a local file fallback to read pending.json and approve entries
/// without needing a WebSocket connection to the gateway. After approving,
/// subsequent CLI commands can connect to the gateway successfully.
fn build_device_approve_cmd() -> String {
    let home = config::OPENCLAW_HOME;
    format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         PENDING=$(node -e 'try{{const d=JSON.parse(require(\"fs\").readFileSync(\"{home}/.openclaw/devices/pending.json\",\"utf8\"));console.log(Object.keys(d).join(\" \"))}}catch(e){{}}' 2>/dev/null); \
         if [ -n \"$PENDING\" ]; then \
           for REQ in $PENDING; do \
             openclaw devices approve \"$REQ\" 2>/dev/null || true; \
           done; \
           echo \"approved pending device(s)\"; \
         fi; true"
    )
}

fn build_cron_add_cmd(
    name: &str,
    schedule: &Option<String>,
    every: &Option<String>,
    message: &str,
    channel: &str,
    to: &Option<String>,
    announce: bool,
) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let mut args = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw cron add"
    );

    args.push_str(&format!(" --name {}", shell_escape(name)));
    args.push_str(&format!(" --message {}", shell_escape(message)));

    match (schedule, every) {
        (Some(expr), _) => args.push_str(&format!(" --cron {}", shell_escape(expr))),
        (_, Some(dur)) => args.push_str(&format!(" --every {}", shell_escape(dur))),
        _ => bail!("Either --schedule (cron expression) or --every (duration) is required"),
    }

    if announce {
        args.push_str(&format!(" --announce --channel {}", shell_escape(channel)));
    }

    if let Some(dest) = to {
        args.push_str(&format!(" --to {}", shell_escape(dest)));
    }

    args.push_str(" 2>&1");
    Ok(args)
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Parse the most-recently-updated recipient for the given channel from a
/// sessions.json string. Returns `None` when no matching session is found.
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

        // Strip channel prefix: "telegram:7547736315" → "7547736315"
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

/// Resolve the delivery `--to` argument from already-fetched sessions JSON.
/// Returns `None` for the "last" channel (OpenClaw handles routing internally).
fn resolve_recipient_from_sessions(
    sessions_json: &str,
    channel: &str,
    to: &Option<String>,
) -> Option<String> {
    if let Some(explicit) = to {
        return Some(explicit.clone());
    }
    if channel == "last" {
        return None;
    }
    if channel == "telegram" || channel == "whatsapp" {
        return parse_best_recipient(sessions_json, channel);
    }
    None
}

/// Add a scheduled message cron job on an OpenClaw instance.
///
/// The gateway agent will receive the message on schedule and announce the
/// response to the specified delivery channel (e.g. telegram, whatsapp).
pub async fn add_message(
    query: &str,
    name: &str,
    schedule: &Option<String>,
    every: &Option<String>,
    message: &str,
    channel: &str,
    to: &Option<String>,
) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Adding scheduled message cron job on {ip}...");
    println!("  Name:    {name}");
    if let Some(expr) = schedule {
        println!("  Cron:    {expr}");
    }
    if let Some(dur) = every {
        println!("  Every:   {dur}");
    }
    println!("  Message: {message}");
    println!("  Channel: {channel}");

    // Determine whether we need to auto-lookup the recipient from sessions.json.
    let needs_lookup = to.is_none() && (channel == "telegram" || channel == "whatsapp");

    // Single SSH session: approve pending devices + (optionally) fetch sessions.json.
    let approve_cmd = build_device_approve_cmd();
    let sessions_cmd = format!("cat {SESSIONS_FILE} 2>/dev/null || echo '{{}}'");

    let resolved_to = if needs_lookup {
        let outputs = ssh_as_openclaw_with_user_multi_async(
            &ip,
            &key,
            vec![approve_cmd, sessions_cmd],
            ssh_user,
        )
        .await?;
        match resolve_recipient_from_sessions(&outputs[1], channel, to) {
            Some(r) => {
                println!("  Auto-detected {channel} recipient: {r}");
                Some(r)
            }
            None => {
                eprintln!(
                    "  Warning: No '{channel}' session found on this instance. \
                     Make sure someone has messaged the bot first, or pass --to <chatId> manually."
                );
                eprintln!("  Falling back to channel routing without --to.");
                None
            }
        }
    } else {
        let _ = ssh_as_openclaw_with_user_async(&ip, &key, &approve_cmd, ssh_user).await;
        to.clone()
    };

    let cmd = build_cron_add_cmd(name, schedule, every, message, channel, &resolved_to, true)?;
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let output = output.trim();

    println!("  {output}");
    if !output.contains("error") && !output.contains("Error") {
        println!("\nCron job '{name}' created. The gateway will send the message on schedule");
        println!("and deliver the response to the '{channel}' channel.");
        println!("\nTip: run `clawmacdo cron-list --instance {query}` to see all jobs.");
    }

    Ok(())
}

pub struct AddToolParams<'a> {
    pub query: &'a str,
    pub name: &'a str,
    pub schedule: &'a Option<String>,
    pub every: &'a Option<String>,
    pub tool: &'a str,
    pub args: &'a str,
    pub channel: &'a str,
    pub to: &'a Option<String>,
}

/// Add a scheduled tool-execution cron job on an OpenClaw instance.
///
/// Sends a message asking the agent to run a specific installed tool, then
/// announces the result to the delivery channel.
pub async fn add_tool(p: AddToolParams<'_>) -> Result<()> {
    let AddToolParams {
        query,
        name,
        schedule,
        every,
        tool,
        args,
        channel,
        to,
    } = p;
    let message = if args.is_empty() {
        format!("Run the {tool} tool and report the results.")
    } else {
        format!("Run the {tool} tool with these inputs: {args}")
    };

    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Adding scheduled tool cron job on {ip}...");
    println!("  Name:    {name}");
    if let Some(expr) = schedule {
        println!("  Cron:    {expr}");
    }
    if let Some(dur) = every {
        println!("  Every:   {dur}");
    }
    println!("  Tool:    {tool}");
    if !args.is_empty() {
        println!("  Args:    {args}");
    }
    println!("  Channel: {channel}");

    // Determine whether we need to auto-lookup the recipient from sessions.json.
    let needs_lookup = to.is_none() && (channel == "telegram" || channel == "whatsapp");

    // Single SSH session: approve pending devices + (optionally) fetch sessions.json.
    let approve_cmd = build_device_approve_cmd();
    let sessions_cmd = format!("cat {SESSIONS_FILE} 2>/dev/null || echo '{{}}'");

    let resolved_to = if needs_lookup {
        let outputs = ssh_as_openclaw_with_user_multi_async(
            &ip,
            &key,
            vec![approve_cmd, sessions_cmd],
            ssh_user,
        )
        .await?;
        match resolve_recipient_from_sessions(&outputs[1], channel, to) {
            Some(r) => {
                println!("  Auto-detected {channel} recipient: {r}");
                Some(r)
            }
            None => {
                eprintln!(
                    "  Warning: No '{channel}' session found on this instance. \
                     Make sure someone has messaged the bot first, or pass --to <chatId> manually."
                );
                eprintln!("  Falling back to channel routing without --to.");
                None
            }
        }
    } else {
        let _ = ssh_as_openclaw_with_user_async(&ip, &key, &approve_cmd, ssh_user).await;
        to.clone()
    };

    let cmd = build_cron_add_cmd(name, schedule, every, &message, channel, &resolved_to, true)?;
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let output = output.trim();

    println!("  {output}");

    if !output.contains("error") && !output.contains("Error") {
        println!("\nCron job '{name}' created. The gateway will run the '{tool}' tool on schedule");
        println!("and deliver the result to the '{channel}' channel.");
        println!("\nTip: run `clawmacdo cron-list --instance {query}` to see all jobs.");
    }

    Ok(())
}

/// List all cron jobs on an OpenClaw instance.
pub async fn list(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    let list_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw cron list 2>&1"
    );

    // Single SSH session: approve pending devices + list crons.
    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![build_device_approve_cmd(), list_cmd],
        ssh_user,
    )
    .await?;
    println!("{}", outputs[1].trim());
    Ok(())
}

/// Remove a cron job by name from an OpenClaw instance.
///
/// Lists jobs first to resolve the name to an ID, then removes by ID.
pub async fn remove(query: &str, name: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Looking up cron job '{name}' on {ip}...");

    let list_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw cron list --json 2>/dev/null"
    );

    // Session 1: approve pending devices + list crons as JSON to resolve name → ID.
    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![build_device_approve_cmd(), list_cmd],
        ssh_user,
    )
    .await?;
    let job_id = parse_job_id_by_name(&outputs[1], name)?;

    println!("Removing cron job '{name}' (id: {job_id})...");

    // Session 2: remove by ID (device is already approved from session 1).
    let rm_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw cron rm {} 2>&1",
        shell_escape(&job_id)
    );
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &rm_cmd, ssh_user).await?;
    println!("{}", output.trim());
    Ok(())
}

fn parse_job_id_by_name(json_out: &str, name: &str) -> Result<String> {
    // `openclaw cron list --json` outputs {"jobs": [...]}.
    // The output may be prefixed with banner text or warnings, so find the
    // first `{` to locate the start of the JSON object.
    let trimmed = json_out.trim();
    let json_str = trimmed.find('{').map(|i| &trimmed[i..]).unwrap_or(trimmed);
    let root: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse cron list output: {e}\n{json_out}"))?;
    let arr = root
        .get("jobs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Expected {{\"jobs\":[...]}} from cron list"))?;
    for job in arr {
        if job.get("name").and_then(|v| v.as_str()) == Some(name) {
            if let Some(id) = job.get("id").and_then(|v| v.as_str()) {
                return Ok(id.to_string());
            }
        }
    }
    bail!("No cron job named '{name}' found on this instance");
}
