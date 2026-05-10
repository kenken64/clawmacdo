use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_multi_async;
use std::path::PathBuf;

const IDENTITY_BLOCK_START: &str = "<!-- clawmacdo:identity:start -->";
const IDENTITY_BLOCK_END: &str = "<!-- clawmacdo:identity:end -->";
const OWNER_BLOCK_START: &str = "<!-- clawmacdo:owner:start -->";
const OWNER_BLOCK_END: &str = "<!-- clawmacdo:owner:end -->";

pub struct OpenclawIdentityParams {
    pub instance: String,
    pub openclaw_name: String,
    pub owner_name: String,
    pub agent: String,
    pub theme: Option<String>,
    pub emoji: Option<String>,
    pub avatar: Option<String>,
}

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

fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn clean_required(flag: &str, value: &str, max_len: usize) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("--{flag} cannot be empty.");
    }
    if trimmed.len() > max_len {
        bail!("--{flag} must be {max_len} bytes or fewer.");
    }
    if trimmed.chars().any(char::is_control) {
        bail!("--{flag} cannot contain control characters or newlines.");
    }
    Ok(trimmed.to_string())
}

fn clean_optional(flag: &str, value: Option<String>, max_len: usize) -> Result<Option<String>> {
    match value {
        Some(v) if v.trim().is_empty() => Ok(None),
        Some(v) => clean_required(flag, &v, max_len).map(Some),
        None => Ok(None),
    }
}

fn clean_agent_id(value: &str) -> Result<String> {
    let agent = clean_required("agent", value, 80)?;
    if !agent
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        bail!("--agent may only contain letters, numbers, dots, underscores, and hyphens.");
    }
    Ok(agent)
}

fn js_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn build_identity_cmd(
    agent: &str,
    openclaw_name: &str,
    theme: Option<&str>,
    emoji: Option<&str>,
    avatar: Option<&str>,
) -> String {
    let mut cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         openclaw agents set-identity --agent {agent} --name {name}",
        home = config::OPENCLAW_HOME,
        agent = shell_escape(agent),
        name = shell_escape(openclaw_name),
    );

    if let Some(theme) = theme {
        cmd.push_str(&format!(" --theme {}", shell_escape(theme)));
    }
    if let Some(emoji) = emoji {
        cmd.push_str(&format!(" --emoji {}", shell_escape(emoji)));
    }
    if let Some(avatar) = avatar {
        cmd.push_str(&format!(" --avatar {}", shell_escape(avatar)));
    }
    cmd.push_str(" --json 2>&1");
    cmd
}

fn build_workspace_cmd(params: &OpenclawIdentityParams) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let agent = js_string(&params.agent)?;
    let openclaw_name = js_string(&params.openclaw_name)?;
    let owner_name = js_string(&params.owner_name)?;
    let theme = match &params.theme {
        Some(v) => js_string(v)?,
        None => "null".to_string(),
    };
    let emoji = match &params.emoji {
        Some(v) => js_string(v)?,
        None => "null".to_string(),
    };

    Ok(format!(
        r#"export HOME="{home}" && \
         node <<'NODE'
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '{home}';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = {agent};
const agentName = {openclaw_name};
const ownerName = {owner_name};
const theme = {theme};
const emoji = {emoji};

function readJson(file) {{
  try {{
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  }} catch (_) {{
    return {{}};
  }}
}}

function expandWorkspace(raw) {{
  let value = typeof raw === 'string' && raw.trim()
    ? raw.trim()
    : path.join(home, '.openclaw', 'workspace');
  if (value === '~') value = home;
  if (value.startsWith('~/')) value = path.join(home, value.slice(2));
  if (value.startsWith('$HOME/')) value = path.join(home, value.slice(6));
  if (value.startsWith('${{HOME}}/')) value = path.join(home, value.slice(8));
  if (!path.isAbsolute(value)) value = path.join(home, value);
  return path.normalize(value);
}}

function escapeRegExp(value) {{
  return value.replace(/[.*+?^${{}}()|[\]\\]/g, '\\$&');
}}

function upsertManagedBlock(file, start, end, block) {{
  let existing = '';
  try {{
    existing = fs.readFileSync(file, 'utf8');
  }} catch (_) {{}}

  const re = new RegExp(escapeRegExp(start) + '[\\s\\S]*?' + escapeRegExp(end) + '\\n?', 'm');
  const body = start + '\n' + block.trimEnd() + '\n' + end + '\n';
  let next;
  if (re.test(existing)) {{
    next = existing.replace(re, body);
  }} else {{
    const trimmed = existing.trimEnd();
    next = body + (trimmed ? '\n' + trimmed + '\n' : '');
  }}
  fs.writeFileSync(file, next, {{ mode: 0o644 }});
}}

const cfg = readJson(configPath);
const agents = cfg.agents || {{}};
const list = Array.isArray(agents.list) ? agents.list : [];
const agent = list.find((item) => item && item.id === agentId);
const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : agents.defaults && agents.defaults.workspace
);

fs.mkdirSync(workspace, {{ recursive: true }});

const identityLines = ['# Agent Identity', '', `Name: ${{agentName}}`];
if (theme) identityLines.push(`Theme: ${{theme}}`);
if (emoji) identityLines.push(`Emoji: ${{emoji}}`);

upsertManagedBlock(
  path.join(workspace, 'IDENTITY.md'),
  '{identity_start}',
  '{identity_end}',
  identityLines.join('\n')
);

upsertManagedBlock(
  path.join(workspace, 'USER.md'),
  '{owner_start}',
  '{owner_end}',
  [
    '# Owner',
    '',
    `The owner of this OpenClaw instance is ${{ownerName}}.`,
    `Address the owner as "${{ownerName}}" unless they ask for a different name.`
  ].join('\n')
);

console.log(`workspace=${{workspace}}`);
console.log('files=IDENTITY.md,USER.md');
NODE"#,
        home = home,
        agent = agent,
        openclaw_name = openclaw_name,
        owner_name = owner_name,
        theme = theme,
        emoji = emoji,
        identity_start = IDENTITY_BLOCK_START,
        identity_end = IDENTITY_BLOCK_END,
        owner_start = OWNER_BLOCK_START,
        owner_end = OWNER_BLOCK_END,
    ))
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

pub async fn run(params: OpenclawIdentityParams) -> Result<()> {
    let params = OpenclawIdentityParams {
        instance: clean_required("instance", &params.instance, 255)?,
        openclaw_name: clean_required("openclaw-name", &params.openclaw_name, 120)?,
        owner_name: clean_required("owner-name", &params.owner_name, 120)?,
        agent: clean_agent_id(&params.agent)?,
        theme: clean_optional("theme", params.theme, 200)?,
        emoji: clean_optional("emoji", params.emoji, 40)?,
        avatar: clean_optional("avatar", params.avatar, 500)?,
    };

    let (ip, key, provider) = find_deploy_record(&params.instance)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Updating OpenClaw identity on {ip}...");
    println!("[1/3] Setting OpenClaw agent identity...");
    println!("[2/3] Writing owner context into the agent workspace...");
    println!("[3/3] Restarting gateway...");

    let identity_cmd = build_identity_cmd(
        &params.agent,
        &params.openclaw_name,
        params.theme.as_deref(),
        params.emoji.as_deref(),
        params.avatar.as_deref(),
    );
    let workspace_cmd = build_workspace_cmd(&params)?;

    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![identity_cmd, workspace_cmd, restart_cmd()],
        ssh_user,
    )
    .await?;

    if !outputs[0].trim().is_empty() {
        println!("  identity: {}", outputs[0].trim());
    }
    if !outputs[1].trim().is_empty() {
        for line in outputs[1].trim().lines() {
            println!("  {line}");
        }
    }
    println!("  {}", outputs[2].trim());

    println!();
    println!("OpenClaw identity updated on {ip}:");
    println!("  Agent: {} ({})", params.openclaw_name, params.agent);
    println!("  Owner: {}", params.owner_name);

    Ok(())
}
