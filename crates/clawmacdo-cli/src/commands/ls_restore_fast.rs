use anyhow::{bail, Context, Result};
use chrono::Utc;
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_cloud::CloudProvider;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_db as db;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_with_user_async, ssh_root_as_async,
};
use clawmacdo_ssh as ssh;
use serde::Serialize;
use std::time::{Duration, Instant};

const SSH_USER: &str = "ubuntu";
const DEFAULT_CHAT_BASE_URL: &str = "http://127.0.0.1:18789/v1";

pub struct LsRestoreFastParams {
    pub snapshot_name: String,
    pub region: String,
    pub size: Option<String>,
    pub telegram_bot_token: String,
    pub openclaw_name: String,
    pub owner_name: String,
    pub avatar_name: Option<String>,
    pub agent: String,
    pub remotion_app_dir: String,
    pub remotion_port: u16,
    pub chat_model: String,
    pub openai_api_key: Option<String>,
    pub voice_gender: String,
    pub telegram_pair_code: Option<String>,
    pub active_timeout_secs: u64,
    pub ssh_timeout_secs: u64,
    pub json: bool,
}

#[derive(Debug, Serialize)]
pub struct LsRestoreFastResult {
    pub ok: bool,
    pub deploy_id: String,
    pub hostname: String,
    pub ip_address: String,
    pub ssh_key_path: String,
    pub snapshot_name: String,
    pub elapsed_ms: u128,
    pub telegram_configured: bool,
    pub telegram_pair_approved: bool,
    pub identity_updated: bool,
    pub remotion_env_updated: bool,
    pub gateway_status: Option<String>,
    pub tailscale_public_url: Option<String>,
    pub remotion_url: Option<String>,
    pub cloudflared_active: bool,
    pub remotion_tunnel_service: Option<String>,
    pub warnings: Vec<String>,
}

fn emit(json: bool, message: impl AsRef<str>) {
    if !json {
        println!("{}", message.as_ref());
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

fn clean_optional_secret(
    flag: &str,
    value: Option<String>,
    max_len: usize,
) -> Result<Option<String>> {
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

fn clean_app_dir(value: &str) -> Result<String> {
    let app_dir = clean_required("remotion-app-dir", value, 512)?;
    let workspace = format!("{}/.openclaw/workspace/", config::OPENCLAW_HOME);
    if !app_dir.starts_with(&workspace) {
        bail!("--remotion-app-dir must be under {workspace}");
    }
    if app_dir.contains("/../") || app_dir.ends_with("/..") {
        bail!("--remotion-app-dir cannot contain '..' path segments.");
    }
    Ok(app_dir)
}

fn clean_voice_gender(value: &str) -> Result<String> {
    let gender = clean_required("voice-gender", value, 16)?.to_ascii_lowercase();
    match gender.as_str() {
        "male" | "female" => Ok(gender),
        _ => bail!("--voice-gender must be either 'male' or 'female'."),
    }
}

fn tts_voice_for_gender(gender: &str) -> &'static str {
    match gender {
        "female" => "nova",
        _ => "onyx",
    }
}

fn clean_pair_code(value: Option<String>) -> Result<Option<String>> {
    match value {
        Some(v) if v.trim().is_empty() => Ok(None),
        Some(v) => {
            let code = v.trim().to_ascii_uppercase();
            if code.len() != 8 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
                bail!("--telegram-pair-code must be 8 alphanumeric characters.");
            }
            Ok(Some(code))
        }
        None => Ok(None),
    }
}

fn js_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn build_fast_configure_cmd(params: &LsRestoreFastParams) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let telegram_bot_token = js_string(&params.telegram_bot_token)?;
    let openclaw_name = js_string(&params.openclaw_name)?;
    let owner_name = js_string(&params.owner_name)?;
    let avatar_name = js_string(
        params
            .avatar_name
            .as_deref()
            .unwrap_or(&params.openclaw_name),
    )?;
    let agent = js_string(&params.agent)?;
    let remotion_app_dir = js_string(&params.remotion_app_dir)?;
    let chat_model = js_string(&params.chat_model)?;
    let openai_api_key = match &params.openai_api_key {
        Some(v) => js_string(v)?,
        None => "null".to_string(),
    };
    let voice_gender = js_string(&params.voice_gender)?;
    let tts_voice = js_string(tts_voice_for_gender(&params.voice_gender))?;
    let telegram_pair_code = params
        .telegram_pair_code
        .as_deref()
        .map(shell_escape)
        .unwrap_or_else(|| "''".to_string());

    let mut cmd = r#"set -e
export HOME="__HOME__"
export PATH="__HOME__/.local/bin:__HOME__/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
if [ ! -S "$XDG_RUNTIME_DIR/bus" ]; then
  dbus-daemon --session --address="$DBUS_SESSION_BUS_ADDRESS" --fork >/dev/null 2>&1 || true
fi

node <<'NODE'
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '__HOME__';
const openclawDir = path.join(home, '.openclaw');
const configPath = path.join(openclawDir, 'openclaw.json');
const botToken = __TELEGRAM_TOKEN_JSON__;
const openclawName = __OPENCLAW_NAME_JSON__;
const ownerName = __OWNER_NAME_JSON__;
const avatarName = __AVATAR_NAME_JSON__;
const agentId = __AGENT_JSON__;
const remotionAppDir = __REMOTION_APP_DIR_JSON__;
const chatModel = __CHAT_MODEL_JSON__;
const requestedOpenaiApiKey = __OPENAI_API_KEY_JSON__;
const voiceGender = __VOICE_GENDER_JSON__;
const ttsVoice = __TTS_VOICE_JSON__;

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

function upsertEnv(file, values) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  let lines = [];
  try {
    lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
    if (lines.length && lines[lines.length - 1] === '') lines.pop();
  } catch (_) {}

  const remaining = new Map(Object.entries(values).filter(([, value]) => value !== null && value !== undefined));
  lines = lines.map((line) => {
    const match = line.match(/^([A-Za-z_][A-Za-z0-9_]*)=/);
    if (!match || !remaining.has(match[1])) return line;
    const value = remaining.get(match[1]);
    remaining.delete(match[1]);
    return `${match[1]}=${value}`;
  });
  for (const [key, value] of remaining) {
    lines.push(`${key}=${value}`);
  }
  fs.writeFileSync(file, lines.join('\n') + '\n', { mode: 0o600 });
  fs.chmodSync(file, 0o600);
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
cfg.channels = object(cfg.channels);
cfg.channels.telegram = object(cfg.channels.telegram);
cfg.channels.telegram.enabled = true;
cfg.channels.telegram.botToken = botToken;
cfg.channels.telegram.dmPolicy = cfg.channels.telegram.dmPolicy || 'pairing';
cfg.channels.telegram.groupPolicy = cfg.channels.telegram.groupPolicy || 'allowlist';
const streaming = cfg.channels.telegram.streaming;
if (streaming !== undefined && (!streaming || typeof streaming !== 'object' || Array.isArray(streaming))) {
  delete cfg.channels.telegram.streaming;
}

cfg.agents = object(cfg.agents);
cfg.agents.defaults = object(cfg.agents.defaults);
if (!Array.isArray(cfg.agents.list)) cfg.agents.list = [];
let agent = cfg.agents.list.find((item) => item && item.id === agentId);
if (!agent) {
  agent = { id: agentId };
  cfg.agents.list.push(agent);
}
agent.identity = object(agent.identity);
agent.identity.name = openclawName;

fs.mkdirSync(path.dirname(configPath), { recursive: true });
if (fs.existsSync(configPath)) {
  fs.copyFileSync(configPath, configPath + '.clawmacdo-fast-restore.bak');
}
fs.writeFileSync(configPath, JSON.stringify(cfg, null, 2) + '\n', { mode: 0o600 });
fs.chmodSync(configPath, 0o600);

for (const file of [
  path.join(openclawDir, 'credentials', 'telegram-pairing.json'),
  path.join(openclawDir, 'credentials', 'telegram-default-allowFrom.json')
]) {
  fs.rmSync(file, { force: true });
}
try {
  for (const name of fs.readdirSync(path.join(openclawDir, 'telegram'))) {
    if (/^update-offset-.*\.json$/.test(name)) {
      fs.rmSync(path.join(openclawDir, 'telegram', name), { force: true });
    }
  }
} catch (_) {}

for (const file of [path.join(openclawDir, '.env'), path.join(openclawDir, 'gateway.env')]) {
  upsertEnv(file, { TELEGRAM_BOT_TOKEN: botToken });
}

const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : cfg.agents.defaults && cfg.agents.defaults.workspace
);
fs.mkdirSync(workspace, { recursive: true });
upsertManagedBlock(
  path.join(workspace, 'IDENTITY.md'),
  '<!-- clawmacdo:identity:start -->',
  '<!-- clawmacdo:identity:end -->',
  ['# Agent Identity', '', `Name: ${openclawName}`].join('\n')
);
upsertManagedBlock(
  path.join(workspace, 'USER.md'),
  '<!-- clawmacdo:owner:start -->',
  '<!-- clawmacdo:owner:end -->',
  [
    '# Owner',
    '',
    `The owner of this OpenClaw instance is ${ownerName}.`,
    `Address the owner as "${ownerName}" unless they ask for a different name.`
  ].join('\n')
);

let stat;
try {
  stat = fs.statSync(remotionAppDir);
} catch (_) {}
if (!stat || !stat.isDirectory()) {
  console.error(`remotion app directory not found: ${remotionAppDir}`);
  process.exit(3);
}

const gatewayToken = cfg.gateway && cfg.gateway.auth && cfg.gateway.auth.token
  ? String(cfg.gateway.auth.token)
  : '';
const remotionEnv = path.join(remotionAppDir, '.env');
let existingOpenaiApiKey = '';
try {
  const line = fs.readFileSync(remotionEnv, 'utf8')
    .split(/\r?\n/)
    .find((item) => item.startsWith('OPENAI_API_KEY='));
  existingOpenaiApiKey = line ? line.slice('OPENAI_API_KEY='.length).trim() : '';
} catch (_) {}
const openaiApiKey = requestedOpenaiApiKey || existingOpenaiApiKey || gatewayToken;

upsertEnv(remotionEnv, {
  CHAT_BASE_URL: '__CHAT_BASE_URL__',
  CHAT_API_KEY: gatewayToken,
  CHAT_MODEL: chatModel,
  VITE_AVATAR_NAME: avatarName,
  OPENAI_API_KEY: openaiApiKey,
  VOICE_GENDER: voiceGender,
  TTS_VOICE: ttsVoice
});

console.log('TELEGRAM_CONFIGURED=true');
console.log('IDENTITY_UPDATED=true');
console.log('REMOTION_ENV_UPDATED=true');
console.log(`REMOTION_ENV=${remotionEnv}`);
console.log(`GATEWAY_TOKEN_PRESENT=${gatewayToken ? 'true' : 'false'}`);
NODE

(timeout 10 openclaw doctor --fix >/dev/null 2>&1 || true)
(systemctl --user daemon-reload 2>/dev/null || true)
(systemctl --user restart openclaw-gateway.service >/dev/null 2>&1 || \
 systemctl --user start openclaw-gateway.service >/dev/null 2>&1 || true)
for i in $(seq 1 10); do
  if curl -fsS --max-time 1 http://127.0.0.1:18789/health >/dev/null 2>&1; then
    echo 'GATEWAY_STATUS=healthy'
    break
  fi
  STATE=$(systemctl --user is-active openclaw-gateway.service 2>/dev/null || true)
  if [ "$STATE" = "active" ] && [ "$i" -ge 3 ]; then
    echo 'GATEWAY_STATUS=active'
    break
  fi
  if [ "$i" = "10" ]; then
    echo 'GATEWAY_STATUS=failed'
    exit 1
  fi
  sleep 1
done

PAIRING_CODE=__PAIRING_CODE_SH__
if [ -n "$PAIRING_CODE" ]; then
  openclaw pairing approve telegram "$PAIRING_CODE" 2>&1
  (openclaw pairing approve telegram "$PAIRING_CODE" --notify >/dev/null 2>&1 &)
  echo 'TELEGRAM_PAIR_APPROVED=true'
else
  echo 'TELEGRAM_PAIR_APPROVED=false'
fi
"#
    .to_string();

    for (needle, value) in [
        ("__HOME__", home.to_string()),
        ("__TELEGRAM_TOKEN_JSON__", telegram_bot_token),
        ("__OPENCLAW_NAME_JSON__", openclaw_name),
        ("__OWNER_NAME_JSON__", owner_name),
        ("__AVATAR_NAME_JSON__", avatar_name),
        ("__AGENT_JSON__", agent),
        ("__REMOTION_APP_DIR_JSON__", remotion_app_dir),
        ("__CHAT_MODEL_JSON__", chat_model),
        ("__OPENAI_API_KEY_JSON__", openai_api_key),
        ("__VOICE_GENDER_JSON__", voice_gender),
        ("__TTS_VOICE_JSON__", tts_voice),
        ("__CHAT_BASE_URL__", DEFAULT_CHAT_BASE_URL.to_string()),
        ("__PAIRING_CODE_SH__", telegram_pair_code),
    ] {
        cmd = cmd.replace(needle, &value);
    }

    Ok(cmd)
}

fn build_tailscale_check_cmd() -> String {
    r#"set +e
STATUS=$(tailscale funnel status 2>&1 || true)
URL=$(printf '%s\n' "$STATUS" | awk '/^https:\/\// { gsub(/:$/, "", $1); print $1; exit }')
if [ -z "$URL" ]; then
  DNS=$(tailscale status --json 2>/dev/null | node -e "let data='';process.stdin.on('data',c=>data+=c);process.stdin.on('end',()=>{try{const v=JSON.parse(data);const dns=(v.Self&&v.Self.DNSName||'').replace(/\.$/,'');process.stdout.write(dns?('https://'+dns):'')}catch(e){}})" 2>/dev/null || true)
  URL="$DNS"
fi
if [ -n "$URL" ]; then
  echo "TAILSCALE_PUBLIC_URL=$URL"
  echo "TAILSCALE_AVAILABLE=true"
else
  echo "TAILSCALE_PUBLIC_URL="
  echo "TAILSCALE_AVAILABLE=false"
fi
"#
    .to_string()
}

fn build_remotion_check_cmd(app_dir: &str) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let app_dir = js_string(app_dir)?;
    let mut cmd = r#"set +e
export HOME="__HOME__"
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
APP_DIR=$(node -e 'process.stdout.write(__APP_DIR_JSON__)')
SERVICE=$(systemctl --user is-active remotion-avatar-tunnel.service 2>/dev/null || true)
PROC=$(pgrep -af 'cloudflared.*(tunnel|127\.0\.0\.1)' 2>/dev/null | head -1 || true)
LOG_FILE="$HOME/.local/state/remotion-avatar/cloudflared.log"
URL=$(grep -Eo 'https://[-a-zA-Z0-9.]+\.trycloudflare\.com' "$LOG_FILE" 2>/dev/null | tail -1 || true)
AVATAR_NAME=$(grep -E '^VITE_AVATAR_NAME=' "$APP_DIR/.env" 2>/dev/null | tail -1 | cut -d= -f2- || true)
echo "REMOTION_TUNNEL_SERVICE=$SERVICE"
if [ "$SERVICE" = active ] || [ -n "$PROC" ]; then
  echo "REMOTION_CLOUDFLARED_OK=true"
else
  echo "REMOTION_CLOUDFLARED_OK=false"
fi
echo "REMOTION_URL=$URL"
echo "REMOTION_AVATAR_NAME=$AVATAR_NAME"
"#
    .to_string();
    for (needle, value) in [
        ("__HOME__", home.to_string()),
        ("__APP_DIR_JSON__", app_dir),
    ] {
        cmd = cmd.replace(needle, &value);
    }
    Ok(cmd)
}

fn line_value(output: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .map(|value| value.trim().to_string())
    })
}

fn line_bool(output: &str, key: &str) -> bool {
    line_value(output, key).as_deref() == Some("true")
}

fn save_deploy_record(
    deploy_id: &str,
    hostname: &str,
    ip: &str,
    region: &str,
    size: &str,
    key_path: &str,
    key_fingerprint: String,
) -> Result<()> {
    let conn = db::init_db().context("Failed to open deployments database")?;
    db::insert_deployment(
        &conn,
        deploy_id,
        "snapshot-restore-fast",
        "",
        "lightsail",
        region,
        size,
        hostname,
    )
    .context("Failed to insert deployment record")?;
    db::update_deployment_status(&conn, deploy_id, "completed", Some(ip), Some(hostname))
        .context("Failed to update deployment status")?;

    let record = DeployRecord {
        id: deploy_id.to_string(),
        provider: Some(CloudProviderType::Lightsail),
        droplet_id: 0,
        instance_id: None,
        hostname: hostname.to_string(),
        ip_address: ip.to_string(),
        region: region.to_string(),
        size: size.to_string(),
        ssh_key_path: key_path.to_string(),
        ssh_key_fingerprint: key_fingerprint,
        ssh_key_id: None,
        resource_group: None,
        backup_restored: None,
        created_at: Utc::now(),
    };
    record.save()?;
    Ok(())
}

pub async fn run(params: LsRestoreFastParams) -> Result<()> {
    let started = Instant::now();
    config::ensure_dirs()?;

    let params = LsRestoreFastParams {
        snapshot_name: clean_required("snapshot-name", &params.snapshot_name, 255)?,
        region: clean_required("region", &params.region, 64)?,
        size: params.size,
        telegram_bot_token: clean_required("telegram-bot-token", &params.telegram_bot_token, 4096)?,
        openclaw_name: clean_required("openclaw-name", &params.openclaw_name, 120)?,
        owner_name: clean_required("owner-name", &params.owner_name, 120)?,
        avatar_name: match params.avatar_name {
            Some(v) if v.trim().is_empty() => None,
            Some(v) => Some(clean_required("avatar-name", &v, 120)?),
            None => None,
        },
        agent: clean_agent_id(&params.agent)?,
        remotion_app_dir: clean_app_dir(&params.remotion_app_dir)?,
        remotion_port: params.remotion_port,
        chat_model: clean_required("chat-model", &params.chat_model, 80)?,
        openai_api_key: clean_optional_secret("openai-api-key", params.openai_api_key, 4096)?,
        voice_gender: clean_voice_gender(&params.voice_gender)?,
        telegram_pair_code: clean_pair_code(params.telegram_pair_code)?,
        active_timeout_secs: params.active_timeout_secs,
        ssh_timeout_secs: params.ssh_timeout_secs,
        json: params.json,
    };

    let deploy_id = uuid::Uuid::new_v4().to_string();
    let hostname = format!("openclaw-{}", &deploy_id[..8]);
    let size = params
        .size
        .clone()
        .unwrap_or_else(|| config::DEFAULT_SIZE.to_string());

    emit(
        params.json,
        format!(
            "Fast Lightsail restore from snapshot {}",
            params.snapshot_name
        ),
    );

    let provider = LightsailCliProvider::new(params.region.clone());
    let bundle_id = provider.get_bundle_id(&size);
    emit(
        params.json,
        format!(
            "[1/6] Resolving snapshot and key: region={}, size={} ({})",
            params.region, size, bundle_id
        ),
    );

    let snap = provider
        .get_snapshot(&params.snapshot_name)
        .context("Snapshot not found")?;
    emit(
        params.json,
        format!(
            "  snapshot={} state={} size={}GB",
            snap.name.as_deref().unwrap_or("?"),
            snap.state.as_deref().unwrap_or("?"),
            snap.size_in_gb.unwrap_or(0)
        ),
    );

    let keypair = ssh::generate_keypair(&deploy_id)?;
    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    let key_info = provider
        .upload_ssh_key(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key")?;

    emit(params.json, format!("[2/6] Creating instance {hostname}"));
    provider
        .create_instance_from_snapshot(&hostname, &params.snapshot_name, &bundle_id, &key_name)
        .context("Failed to create instance from snapshot")?;

    emit(
        params.json,
        format!(
            "[3/6] Waiting for Lightsail running state ({}s budget)",
            params.active_timeout_secs
        ),
    );
    let instance = provider
        .wait_for_active(&hostname, params.active_timeout_secs)
        .await
        .context("Instance did not become active before the fast restore deadline")?;
    let ip = instance.public_ip.unwrap_or_else(|| "unknown".into());
    emit(params.json, format!("  active at {ip}"));

    emit(
        params.json,
        format!(
            "[4/6] Waiting for SSH as {SSH_USER} ({}s budget)",
            params.ssh_timeout_secs
        ),
    );
    ssh::wait_for_ssh(
        &ip,
        &keypair.private_key_path,
        Duration::from_secs(params.ssh_timeout_secs),
        Some(SSH_USER),
    )
    .await
    .context("Lightsail instance became active but SSH did not become ready")?;

    let key_path = keypair.private_key_path.display().to_string();
    save_deploy_record(
        &deploy_id,
        &hostname,
        &ip,
        &params.region,
        &size,
        &key_path,
        key_info.fingerprint.unwrap_or_default(),
    )?;

    emit(
        params.json,
        "[5/6] Applying Telegram, identity, Remotion env, and gateway restart in one SSH session",
    );
    let setup_out = ssh_as_openclaw_with_user_async(
        &ip,
        &keypair.private_key_path,
        &build_fast_configure_cmd(&params)?,
        SSH_USER,
    )
    .await
    .context("Fast post-restore OpenClaw setup failed")?;

    for line in setup_out.lines() {
        if !params.json
            && (line.starts_with("TELEGRAM_")
                || line.starts_with("IDENTITY_")
                || line.starts_with("REMOTION_")
                || line.starts_with("GATEWAY_"))
        {
            println!("  {line}");
        }
    }

    emit(
        params.json,
        "[6/6] Checking Tailscale public URL and cloudflared Remotion tunnel in parallel",
    );
    let tailscale_check_cmd = build_tailscale_check_cmd();
    let remotion_check_cmd = build_remotion_check_cmd(&params.remotion_app_dir)?;
    let tailscale_check = ssh_root_as_async(
        &ip,
        &keypair.private_key_path,
        &tailscale_check_cmd,
        SSH_USER,
    );
    let remotion_check = ssh_as_openclaw_with_user_async(
        &ip,
        &keypair.private_key_path,
        &remotion_check_cmd,
        SSH_USER,
    );
    let (tailscale_result, remotion_result) = tokio::join!(tailscale_check, remotion_check);

    let mut warnings = Vec::new();
    let tailscale_out = match tailscale_result {
        Ok(out) => out,
        Err(err) => {
            warnings.push(format!("Tailscale check failed: {err}"));
            String::new()
        }
    };
    let remotion_out = match remotion_result {
        Ok(out) => out,
        Err(err) => {
            warnings.push(format!("Remotion cloudflared check failed: {err}"));
            String::new()
        }
    };

    let result = LsRestoreFastResult {
        ok: true,
        deploy_id,
        hostname,
        ip_address: ip,
        ssh_key_path: key_path,
        snapshot_name: params.snapshot_name,
        elapsed_ms: started.elapsed().as_millis(),
        telegram_configured: line_bool(&setup_out, "TELEGRAM_CONFIGURED"),
        telegram_pair_approved: line_bool(&setup_out, "TELEGRAM_PAIR_APPROVED"),
        identity_updated: line_bool(&setup_out, "IDENTITY_UPDATED"),
        remotion_env_updated: line_bool(&setup_out, "REMOTION_ENV_UPDATED"),
        gateway_status: line_value(&setup_out, "GATEWAY_STATUS"),
        tailscale_public_url: line_value(&tailscale_out, "TAILSCALE_PUBLIC_URL")
            .filter(|value| !value.is_empty()),
        remotion_url: line_value(&remotion_out, "REMOTION_URL").filter(|value| !value.is_empty()),
        cloudflared_active: line_bool(&remotion_out, "REMOTION_CLOUDFLARED_OK"),
        remotion_tunnel_service: line_value(&remotion_out, "REMOTION_TUNNEL_SERVICE")
            .filter(|value| !value.is_empty()),
        warnings,
    };

    if params.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!();
    println!(
        "Fast Lightsail restore complete in {:.1}s",
        result.elapsed_ms as f64 / 1000.0
    );
    println!("  Deploy ID:  {}", result.deploy_id);
    println!("  Hostname:   {}", result.hostname);
    println!("  IP Address: {}", result.ip_address);
    println!("  SSH Key:    {}", result.ssh_key_path);
    println!(
        "  Gateway:    {}",
        result.gateway_status.as_deref().unwrap_or("unknown")
    );
    if let Some(url) = &result.tailscale_public_url {
        println!("  Tailscale:  {url}");
    } else {
        println!("  Tailscale:  not available");
    }
    if let Some(url) = &result.remotion_url {
        println!("  Remotion:   {url}");
    } else {
        println!("  Remotion:   cloudflared URL not found");
    }
    for warning in &result.warnings {
        println!("  Warning: {warning}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_params() -> LsRestoreFastParams {
        LsRestoreFastParams {
            snapshot_name: "snap".into(),
            region: "ap-southeast-1".into(),
            size: None,
            telegram_bot_token: "123456:abcdef".into(),
            openclaw_name: "John".into(),
            owner_name: "Kenny".into(),
            avatar_name: Some("John".into()),
            agent: "main".into(),
            remotion_app_dir: "/home/openclaw/.openclaw/workspace/remotion-3d-AI-avatar".into(),
            remotion_port: 3002,
            chat_model: "openclaw".into(),
            openai_api_key: None,
            voice_gender: "male".into(),
            telegram_pair_code: None,
            active_timeout_secs: 120,
            ssh_timeout_secs: 30,
            json: false,
        }
    }

    #[test]
    fn fast_configure_cmd_batches_requested_mutations() {
        let cmd = build_fast_configure_cmd(&test_params()).unwrap();
        assert!(cmd.contains("TELEGRAM_BOT_TOKEN"));
        assert!(cmd.contains("delete cfg.channels.telegram.streaming"));
        assert!(cmd.contains("VITE_AVATAR_NAME"));
        assert!(cmd.contains("CHAT_BASE_URL"));
        assert!(cmd.contains("openclaw-gateway.service"));
    }

    #[test]
    fn line_helpers_parse_key_values() {
        let output = "REMOTION_URL=https://demo.trycloudflare.com\nREMOTION_CLOUDFLARED_OK=true\n";
        assert_eq!(
            line_value(output, "REMOTION_URL").as_deref(),
            Some("https://demo.trycloudflare.com")
        );
        assert!(line_bool(output, "REMOTION_CLOUDFLARED_OK"));
    }
}
