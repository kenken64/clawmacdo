use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
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

    let cmd = build_cron_add_cmd(name, schedule, every, message, channel, to, true)?;
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let output = output.trim();

    if output.contains("error") || output.contains("Error") {
        println!("  {output}");
    } else {
        println!("  {output}");
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

    let cmd = build_cron_add_cmd(name, schedule, every, &message, channel, to, true)?;
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

    let cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw cron list 2>&1"
    );

    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    println!("{}", output.trim());
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

    // Resolve name → ID
    let list_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw cron list --json 2>&1"
    );
    let list_out = ssh_as_openclaw_with_user_async(&ip, &key, &list_cmd, ssh_user).await?;
    let job_id = parse_job_id_by_name(&list_out, name)?;

    println!("Removing cron job '{name}' (id: {job_id})...");

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
    // `openclaw cron list --json` outputs {"jobs": [...]}
    let root: serde_json::Value = serde_json::from_str(json_out.trim())
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
