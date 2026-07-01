use anyhow::{bail, Context, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use serde_json::Value;
use std::path::PathBuf;

pub struct GyneConsumerProfileParams {
    pub instance: String,
    pub agent: String,
    pub project: String,
    pub name: String,
    pub task_stream: Option<String>,
    pub restart: bool,
    pub service: Option<String>,
    pub json: bool,
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
        Some(provider)
            if provider.eq_ignore_ascii_case("lightsail")
                || provider.eq_ignore_ascii_case("hermes-lightsail") =>
        {
            "ubuntu"
        }
        Some(provider) if provider.eq_ignore_ascii_case("azure") => "azureuser",
        _ => "root",
    }
}

fn clean_instance(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--instance cannot be empty.");
    }
    if value.len() > 255 {
        bail!("--instance must be 255 bytes or fewer.");
    }
    if value.chars().any(char::is_control) {
        bail!("--instance cannot contain control characters.");
    }
    Ok(value.to_string())
}

fn clean_agent_id(value: &str) -> Result<String> {
    let agent = value.trim();
    if agent.is_empty() {
        bail!("--agent cannot be empty.");
    }
    if agent.len() > 80 {
        bail!("--agent must be 80 bytes or fewer.");
    }
    if !agent
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        bail!("--agent may only contain letters, numbers, dots, underscores, and hyphens.");
    }
    Ok(agent.to_string())
}

fn clean_project_slug(value: &str) -> Result<String> {
    let project = value.trim();
    if project.is_empty() {
        bail!("--project cannot be empty.");
    }
    if project.len() > 120 {
        bail!("--project must be 120 bytes or fewer.");
    }
    if project == "." || project == ".." {
        bail!("--project cannot be '.' or '..'.");
    }
    if !project
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        bail!("--project may only contain letters, numbers, dots, underscores, and hyphens.");
    }
    Ok(project.to_string())
}

fn clean_consumer_profile_name(value: &str) -> Result<String> {
    let name = value.trim();
    if name.is_empty() {
        bail!("--name cannot be empty.");
    }
    if name.len() > 80 {
        bail!("--name must be 80 bytes or fewer.");
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        bail!("--name may only contain letters, numbers, dots, underscores, and hyphens.");
    }
    Ok(name.to_string())
}

fn clean_service_unit(value: &str) -> Result<String> {
    let unit = value.trim();
    if unit.is_empty() {
        bail!("--service cannot be empty.");
    }
    if unit.len() > 128 {
        bail!("--service must be 128 bytes or fewer.");
    }
    // Restrict to systemd unit-name characters so the value is safe to embed in the restart shell.
    if !unit
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'@' | b'\\'))
    {
        bail!("--service may only contain letters, numbers, dots, underscores, hyphens, @, and backslashes.");
    }
    Ok(unit.to_string())
}

/// Builds the shell that restarts the Gyne consumer `--user` systemd unit so it re-registers under the
/// freshly written CONSUMER_NAME. When no unit is given it auto-detects a gyne/consumer service
/// (excluding the gateway). Emits a single JSON line on stdout describing the outcome.
fn build_restart_cmd(service: Option<&str>) -> String {
    let explicit = service.unwrap_or("");
    format!(
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus; \
unit='{explicit}'; \
if [ -z \"$unit\" ]; then \
  unit=$(systemctl --user list-unit-files --type=service --no-legend 2>/dev/null | awk '{{print $1}}' | grep -iE 'gyne|consumer' | grep -viE 'gateway' | head -n1); \
fi; \
if [ -z \"$unit\" ]; then \
  printf '{{\"restart\":\"skipped\",\"reason\":\"no_gyne_service_found\"}}\\n'; \
else \
  systemctl --user daemon-reload 2>/dev/null || true; \
  systemctl --user restart \"$unit\" 2>/dev/null || systemctl --user start \"$unit\" 2>/dev/null || true; \
  for i in 1 2 3; do s=$(systemctl --user is-active \"$unit\" 2>/dev/null); [ \"$s\" = active ] && break; sleep 1; done; \
  printf '{{\"restart\":\"%s\",\"unit\":\"%s\"}}\\n' \"$(systemctl --user is-active \"$unit\" 2>/dev/null || echo unknown)\" \"$unit\"; \
fi"
    )
}

fn clean_task_stream(value: &str) -> Result<String> {
    let stream = value.trim();
    if stream.is_empty() {
        bail!("--task-stream cannot be empty.");
    }
    if stream.len() > 255 {
        bail!("--task-stream must be 255 bytes or fewer.");
    }
    if !stream
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
    {
        bail!("--task-stream may only contain letters, numbers, dots, underscores, hyphens, and colons.");
    }
    Ok(stream.to_string())
}

fn js_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn remote_json_value(output: &str) -> Result<Value> {
    serde_json::from_str(output.trim()).context("Failed to parse Gyne consumer profile JSON")
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn handle_remote_status(value: &Value, json: bool) -> Result<()> {
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        if json {
            print_json(value)?;
        }
        let message = value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("remote Gyne consumer profile update failed");
        bail!("{message}");
    }
    Ok(())
}

fn build_update_cmd(
    agent: &str,
    project: &str,
    name: &str,
    task_stream: Option<&str>,
) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let task_stream = match task_stream {
        Some(value) => js_string(value)?,
        None => "null".to_string(),
    };

    let mut template = r#"set -e
export HOME="__HOME__"
export PATH="__HOME__/.local/bin:__HOME__/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"

node <<'NODE'
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '__HOME__';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = __AGENT_JSON__;
const project = __PROJECT_JSON__;
const consumerName = __NAME_JSON__;
const taskStreamOverride = __TASK_STREAM_JSON__;

function fail(code, message, extra = {}) {
  console.log(JSON.stringify({ ok: false, error: { code, message }, ...extra }));
  process.exit(0);
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (_) {
    return {};
  }
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

function isWithin(root, target) {
  const rel = path.relative(root, target);
  return rel === '' || (!!rel && !rel.startsWith('..') && !path.isAbsolute(rel));
}

function envRegex(key) {
  return new RegExp(`^${key}=.*$`, 'm');
}

function readEnvValue(text, key) {
  const match = text.match(new RegExp(`^\\s*${key}\\s*=\\s*(.*)$`, 'm'));
  if (!match) return null;
  let value = match[1].trim();
  if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
    value = value.slice(1, -1);
  }
  return value;
}

function setEnvValue(text, key, value) {
  const line = `${key}=${value}`;
  const re = envRegex(key);
  if (re.test(text)) return text.replace(re, line);
  return text + (text.length && !text.endsWith('\n') ? '\n' : '') + line + '\n';
}

function validateTaskStream(value) {
  return /^[A-Za-z0-9][A-Za-z0-9._:-]{0,254}$/.test(value);
}

const cfg = readJson(configPath);
const agents = cfg.agents || {};
const list = Array.isArray(agents.list) ? agents.list : [];
const agent = list.find((item) => item && item.id === agentId);
const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : agents.defaults && agents.defaults.workspace
);

if (!fs.existsSync(workspace) || !fs.statSync(workspace).isDirectory()) {
  fail('workspace_not_found', `OpenClaw workspace not found: ${workspace}`);
}

const workspaceReal = fs.realpathSync(workspace);
const projectDir = path.resolve(workspaceReal, project);
if (!isWithin(workspaceReal, projectDir)) {
  fail('path_escape', 'Gyne project path escapes the OpenClaw workspace.', { project });
}
if (!fs.existsSync(projectDir)) {
  fail('project_not_found', `Gyne project not found: ${projectDir}`, { project });
}
const projectStat = fs.lstatSync(projectDir);
if (projectStat.isSymbolicLink() || !projectStat.isDirectory()) {
  fail('invalid_project', `Gyne project is not a regular directory: ${projectDir}`, { project });
}
const projectReal = fs.realpathSync(projectDir);
if (!isWithin(workspaceReal, projectReal)) {
  fail('path_escape', 'Gyne project real path escapes the OpenClaw workspace.', { project });
}

const envPath = path.join(projectReal, '.env');
if (!fs.existsSync(envPath)) {
  fail('env_not_found', `Gyne .env not found: ${envPath}`, { project });
}
const envStat = fs.lstatSync(envPath);
if (envStat.isSymbolicLink() || !envStat.isFile()) {
  fail('invalid_env', `Gyne .env is not a regular file: ${envPath}`, { project });
}
const envReal = fs.realpathSync(envPath);
if (!isWithin(projectReal, envReal)) {
  fail('path_escape', 'Gyne .env real path escapes the project directory.', { project });
}

const original = fs.readFileSync(envReal, 'utf8');
const previousName = readEnvValue(original, 'CONSUMER_NAME');
const previousTaskStream = readEnvValue(original, 'CONSUMER_TASK_STREAM');
const taskStream = taskStreamOverride || readEnvValue(original, 'TASK_STREAM') || 'openclaw:tasks';
if (!validateTaskStream(taskStream)) {
  fail('invalid_task_stream', 'TASK_STREAM is missing or contains unsupported characters; pass --task-stream.', {
    task_stream: taskStream
  });
}

const consumerTaskStream = `${taskStream}:${consumerName}`;
let next = setEnvValue(original, 'CONSUMER_NAME', consumerName);
next = setEnvValue(next, 'CONSUMER_TASK_STREAM', consumerTaskStream);

const stamp = new Date().toISOString().replace(/[-:.TZ]/g, '');
const backup = `${envReal}.clawmacdo-gyne-${stamp}.bak`;
fs.copyFileSync(envReal, backup);
try { fs.chmodSync(backup, 0o600); } catch (_) {}
fs.writeFileSync(envReal, next, { mode: 0o600 });
try { fs.chmodSync(envReal, 0o600); } catch (_) {}

console.log(JSON.stringify({
  ok: true,
  workspace: workspaceReal,
  project,
  env_file: envReal,
  backup,
  previous_consumer_name: previousName,
  consumer_name: consumerName,
  task_stream: taskStream,
  previous_consumer_task_stream: previousTaskStream,
  consumer_task_stream: consumerTaskStream
}));
NODE
"#
    .to_string();

    for (needle, value) in [
        ("__HOME__", home.to_string()),
        ("__AGENT_JSON__", js_string(agent)?),
        ("__PROJECT_JSON__", js_string(project)?),
        ("__NAME_JSON__", js_string(name)?),
        ("__TASK_STREAM_JSON__", task_stream),
    ] {
        template = template.replace(needle, &value);
    }

    Ok(template)
}

pub async fn run(params: GyneConsumerProfileParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let agent = clean_agent_id(&params.agent)?;
    let project = clean_project_slug(&params.project)?;
    let name = clean_consumer_profile_name(&params.name)?;
    let service = params
        .service
        .as_deref()
        .map(clean_service_unit)
        .transpose()?;
    let task_stream = params
        .task_stream
        .as_deref()
        .map(clean_task_stream)
        .transpose()?;

    let (ip, key, provider) = find_deploy_record(&instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let cmd = build_update_cmd(&agent, &project, &name, task_stream.as_deref())?;

    println!("Updating Gyne consumer profile on {ip}...");
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let mut value = remote_json_value(&output)?;
    handle_remote_status(&value, params.json)?;

    // Restart the consumer so it re-registers under the new CONSUMER_NAME; editing .env alone is not
    // enough while the systemd service is already running.
    let restart_value = if params.restart {
        if !params.json {
            println!("Restarting Gyne consumer service on {ip}...");
        }
        let restart_cmd = build_restart_cmd(service.as_deref());
        match ssh_as_openclaw_with_user_async(&ip, &key, &restart_cmd, ssh_user).await {
            Ok(restart_output) => remote_json_value(&restart_output).ok(),
            Err(err) => {
                if !params.json {
                    println!("  Warning: consumer restart failed: {err}");
                }
                None
            }
        }
    } else {
        None
    };

    if params.json {
        if let (Some(object), Some(restart)) = (value.as_object_mut(), restart_value.as_ref()) {
            object.insert("restart".to_string(), restart.clone());
        }
        return print_json(&value);
    }

    println!(
        "  Project: {}",
        value
            .get("project")
            .and_then(Value::as_str)
            .unwrap_or(&project)
    );
    println!(
        "  Env file: {}",
        value.get("env_file").and_then(Value::as_str).unwrap_or("")
    );
    println!(
        "  Backup: {}",
        value.get("backup").and_then(Value::as_str).unwrap_or("")
    );
    println!(
        "  CONSUMER_NAME: {} -> {}",
        value
            .get("previous_consumer_name")
            .and_then(Value::as_str)
            .unwrap_or("(missing)"),
        value
            .get("consumer_name")
            .and_then(Value::as_str)
            .unwrap_or(&name)
    );
    println!(
        "  CONSUMER_TASK_STREAM: {} -> {}",
        value
            .get("previous_consumer_task_stream")
            .and_then(Value::as_str)
            .unwrap_or("(missing)"),
        value
            .get("consumer_task_stream")
            .and_then(Value::as_str)
            .unwrap_or("")
    );
    match &restart_value {
        Some(restart) => {
            let state = restart
                .get("restart")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            match restart.get("unit").and_then(Value::as_str) {
                Some(unit) => println!("Gyne consumer profile updated. Consumer restarted ({state}, unit: {unit})."),
                None => println!(
                    "Gyne consumer profile updated, but no Gyne consumer service was found to restart. Pass --service <unit> or restart it manually."
                ),
            }
        }
        None if params.restart => {
            println!("Gyne consumer profile updated. Restart attempted (status unknown).")
        }
        None => println!(
            "Gyne consumer profile updated. Restart the Gyne consumer process if it is already running (--no-restart was set)."
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_profile_name_accepts_expected_names() {
        assert_eq!(
            clean_consumer_profile_name("consumer-3").unwrap(),
            "consumer-3"
        );
        assert_eq!(
            clean_consumer_profile_name("local.profile_01").unwrap(),
            "local.profile_01"
        );
    }

    #[test]
    fn consumer_profile_name_rejects_env_unsafe_values() {
        assert!(clean_consumer_profile_name("").is_err());
        assert!(clean_consumer_profile_name("consumer:3").is_err());
        assert!(clean_consumer_profile_name("consumer 3").is_err());
        assert!(clean_consumer_profile_name("consumer\n3").is_err());
    }

    #[test]
    fn task_stream_accepts_redis_style_stream() {
        assert_eq!(
            clean_task_stream("openclaw:tasks").unwrap(),
            "openclaw:tasks"
        );
        assert!(clean_task_stream("openclaw/tasks").is_err());
    }

    #[test]
    fn update_cmd_targets_only_gyne_consumer_keys() {
        let script = build_update_cmd("main", "gyne-agent", "consumer-4", None).unwrap();
        assert!(script.contains("CONSUMER_NAME"));
        assert!(script.contains("CONSUMER_TASK_STREAM"));
        assert!(script.contains("TASK_STREAM"));
        assert!(script.contains("gyne-agent"));
        assert!(script.contains("consumer-4"));
        assert!(!script.contains("REDIS_URL="));
    }

    #[test]
    fn service_unit_accepts_expected_and_rejects_unsafe() {
        assert_eq!(
            clean_service_unit("gyne-agent.service").unwrap(),
            "gyne-agent.service"
        );
        assert_eq!(
            clean_service_unit("gyne@main.service").unwrap(),
            "gyne@main.service"
        );
        assert!(clean_service_unit("").is_err());
        assert!(clean_service_unit("gyne; rm -rf /").is_err());
        assert!(clean_service_unit("gyne$(id)").is_err());
    }

    #[test]
    fn restart_cmd_auto_detects_when_no_service_and_uses_override() {
        let auto = build_restart_cmd(None);
        assert!(auto.contains("systemctl --user restart"));
        assert!(auto.contains("list-unit-files"));
        assert!(auto.contains("gyne|consumer"));
        assert!(auto.contains("gateway")); // excluded via grep -v
        assert!(auto.contains("no_gyne_service_found"));

        let explicit = build_restart_cmd(Some("gyne-agent.service"));
        assert!(explicit.contains("unit='gyne-agent.service'"));
    }

    #[test]
    fn ssh_user_maps_non_root_clouds() {
        assert_eq!(ssh_user_for_provider(&Some("lightsail".into())), "ubuntu");
        assert_eq!(
            ssh_user_for_provider(&Some("hermes-lightsail".into())),
            "ubuntu"
        );
        assert_eq!(ssh_user_for_provider(&Some("azure".into())), "azureuser");
        assert_eq!(ssh_user_for_provider(&Some("digitalocean".into())), "root");
        assert_eq!(ssh_user_for_provider(&None), "root");
    }
}
