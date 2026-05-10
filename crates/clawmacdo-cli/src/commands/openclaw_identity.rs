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
    let avatar = match &params.avatar {
        Some(v) => js_string(v)?,
        None => "null".to_string(),
    };

    let mut cmd = r#"export HOME="__OPENCLAW_HOME__" && \
         node <<'NODE'
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '__OPENCLAW_HOME__';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = __AGENT_JSON__;
const agentName = __OPENCLAW_NAME_JSON__;
const ownerName = __OWNER_NAME_JSON__;
const theme = __THEME_JSON__;
const emoji = __EMOJI_JSON__;
const avatar = __AVATAR_JSON__;

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

function expandWorkspace(raw) {
  let value = typeof raw === 'string' && raw.trim()
    ? raw.trim()
    : path.join(home, '.openclaw', 'workspace');
  if (value === '~') value = home;
  if (value.startsWith('~/')) value = path.join(home, value.slice(2));
  if (value.startsWith('$HOME/')) value = path.join(home, value.slice(6));
  if (value.startsWith('${HOME}/')) value = path.join(home, value.slice(8));
  if (!path.isAbsolute(value)) value = path.join(home, value);
  return path.normalize(value);
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function upsertManagedBlock(file, start, end, block) {
  let existing = '';
  try {
    existing = fs.readFileSync(file, 'utf8');
  } catch (_) {}

  const re = new RegExp(escapeRegExp(start) + '[\\s\\S]*?' + escapeRegExp(end) + '\\n?', 'm');
  const body = start + '\n' + block.trimEnd() + '\n' + end + '\n';
  let next;
  if (re.test(existing)) {
    next = existing.replace(re, body);
  } else {
    const trimmed = existing.trimEnd();
    next = body + (trimmed ? '\n' + trimmed + '\n' : '');
  }
  fs.writeFileSync(file, next, { mode: 0o644 });
}

const cfg = readJson(configPath);
cfg.agents = object(cfg.agents);
cfg.agents.defaults = object(cfg.agents.defaults);
if (!Array.isArray(cfg.agents.list)) cfg.agents.list = [];

let agent = cfg.agents.list.find((item) => item && item.id === agentId);
if (!agent) {
  agent = { id: agentId };
  cfg.agents.list.push(agent);
}

agent.identity = object(agent.identity);
agent.identity.name = agentName;

function applyOptional(key, value) {
  if (!value) return;
  agent.identity[key] = value;
}

applyOptional('theme', theme);
applyOptional('emoji', emoji);
applyOptional('avatar', avatar);

const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : cfg.agents.defaults && cfg.agents.defaults.workspace
);

fs.mkdirSync(path.dirname(configPath), { recursive: true });
if (fs.existsSync(configPath)) {
  fs.copyFileSync(configPath, configPath + '.clawmacdo-identity.bak');
}
fs.writeFileSync(configPath, JSON.stringify(cfg, null, 2) + '\n', { mode: 0o600 });
fs.mkdirSync(workspace, { recursive: true });

const identityLines = ['# Agent Identity', '', `Name: ${agentName}`];
if (theme) identityLines.push(`Theme: ${theme}`);
if (emoji) identityLines.push(`Emoji: ${emoji}`);
if (avatar) identityLines.push(`Avatar: ${avatar}`);

upsertManagedBlock(
  path.join(workspace, 'IDENTITY.md'),
  '__IDENTITY_START__',
  '__IDENTITY_END__',
  identityLines.join('\n')
);

upsertManagedBlock(
  path.join(workspace, 'USER.md'),
  '__OWNER_START__',
  '__OWNER_END__',
  [
    '# Owner',
    '',
    `The owner of this OpenClaw instance is ${ownerName}.`,
    `Address the owner as "${ownerName}" unless they ask for a different name.`
  ].join('\n')
);

console.log('identity=config');
console.log(`workspace=${workspace}`);
console.log('files=IDENTITY.md,USER.md');
NODE"#
        .to_string();

    for (needle, value) in [
        ("__OPENCLAW_HOME__", home.to_string()),
        ("__AGENT_JSON__", agent),
        ("__OPENCLAW_NAME_JSON__", openclaw_name),
        ("__OWNER_NAME_JSON__", owner_name),
        ("__THEME_JSON__", theme),
        ("__EMOJI_JSON__", emoji),
        ("__AVATAR_JSON__", avatar),
        ("__IDENTITY_START__", IDENTITY_BLOCK_START.to_string()),
        ("__IDENTITY_END__", IDENTITY_BLOCK_END.to_string()),
        ("__OWNER_START__", OWNER_BLOCK_START.to_string()),
        ("__OWNER_END__", OWNER_BLOCK_END.to_string()),
    ] {
        cmd = cmd.replace(needle, &value);
    }

    Ok(cmd)
}

fn restart_cmd() -> String {
    "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
     (systemctl --user daemon-reload 2>/dev/null || true) && \
     (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
      systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
     for i in $(seq 1 12); do \
       if curl -fsS --max-time 1 http://127.0.0.1:18789/health >/dev/null 2>&1; then echo 'gateway: healthy'; exit 0; fi; \
       STATE=$(systemctl --user is-active openclaw-gateway.service 2>/dev/null || true); \
       if [ \"$STATE\" = \"active\" ] && [ \"$i\" -ge 2 ]; then echo 'gateway: active'; exit 0; fi; \
       sleep 1; \
     done; \
     echo 'gateway: FAILED - service not healthy after restart'; exit 1"
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
    println!("[1/2] Updating identity config and workspace files...");
    println!("[2/2] Restarting gateway...");

    let workspace_cmd = build_workspace_cmd(&params)?;

    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![workspace_cmd, restart_cmd()],
        ssh_user,
    )
    .await?;

    if !outputs[0].trim().is_empty() {
        for line in outputs[0].trim().lines() {
            println!("  {line}");
        }
    }
    println!("  {}", outputs[1].trim());

    println!();
    println!("OpenClaw identity updated on {ip}:");
    println!("  Agent: {} ({})", params.openclaw_name, params.agent);
    println!("  Owner: {}", params.owner_name);

    Ok(())
}
