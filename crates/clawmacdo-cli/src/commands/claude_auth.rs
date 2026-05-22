use anyhow::{bail, Context, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use serde_json::Value;
use std::path::PathBuf;

pub struct ClaudeAuthStartParams {
    pub instance: String,
    pub mode: String,
    pub email: Option<String>,
    pub sso: bool,
    pub wait_secs: u64,
    pub json: bool,
}

pub struct ClaudeAuthStatusParams {
    pub instance: String,
    pub json: bool,
}

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

fn clean_mode(value: &str) -> Result<String> {
    match value.trim() {
        "claudeai" | "console" => Ok(value.trim().to_string()),
        _ => bail!("--mode must be claudeai or console."),
    }
}

fn clean_email(value: Option<String>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > 320 {
        bail!("--email must be 320 bytes or fewer.");
    }
    if trimmed.chars().any(char::is_control) {
        bail!("--email cannot contain control characters.");
    }
    Ok(Some(trimmed.to_string()))
}

fn js_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn js_optional_string(value: Option<&str>) -> Result<String> {
    match value {
        Some(value) => Ok(serde_json::to_string(value)?),
        None => Ok("null".to_string()),
    }
}

fn remote_json_value(output: &str, label: &str) -> Result<Value> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        bail!("Remote {label} command returned no JSON.");
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => Ok(value),
        Err(first_err) => {
            let start = trimmed.find('{');
            let end = trimmed.rfind('}');
            if let (Some(start), Some(end)) = (start, end) {
                if start < end {
                    return serde_json::from_str::<Value>(&trimmed[start..=end])
                        .with_context(|| format!("Failed to parse remote {label} JSON"));
                }
            }
            Err(first_err).with_context(|| format!("Failed to parse remote {label} JSON"))
        }
    }
}

fn insert_context(value: &mut Value, instance: &str, ip: &str) {
    if let Value::Object(map) = value {
        map.insert("instance".to_string(), Value::String(instance.to_string()));
        map.insert("ip".to_string(), Value::String(ip.to_string()));
    }
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn value_bool(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn value_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn build_start_cmd(mode: &str, email: Option<&str>, sso: bool, wait_secs: u64) -> Result<String> {
    let wait_ms = wait_secs.saturating_mul(1000).clamp(1_000, 120_000);
    let mut cmd = r#"export HOME="__OPENCLAW_HOME__" && \
         node <<'NODE'
const cp = require('child_process');
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '__OPENCLAW_HOME__';
const mode = __MODE_JSON__;
const email = __EMAIL_JSON__;
const useSso = __SSO_BOOL__;
const waitMs = __WAIT_MS__;
const stateDir = path.join(home, '.openclaw', 'claude-auth');
const logPath = path.join(stateDir, 'login.log');
const pidPath = path.join(stateDir, 'login.pid');
const urlPath = path.join(stateDir, 'login-url.txt');

const env = {
  ...process.env,
  HOME: home,
  PATH: `${home}/.local/bin:${home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin`,
  TERM: 'dumb',
  NO_COLOR: '1',
  FORCE_COLOR: '0',
};

function finish(value) {
  console.log(JSON.stringify(value, null, 2));
}

function commandOk(command) {
  const result = cp.spawnSync('bash', ['-lc', command], {
    env,
    encoding: 'utf8',
    timeout: 10000,
  });
  return result.status === 0;
}

function stripAnsi(value) {
  return String(value || '').replace(/\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])/g, '');
}

function readFile(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch (_) {
    return '';
  }
}

function outputTail(value, limit = 4000) {
  const clean = stripAnsi(value);
  return clean.length > limit ? clean.slice(clean.length - limit) : clean;
}

function extractLoginUrl(value) {
  const clean = stripAnsi(value);
  const matches = clean.match(/https?:\/\/[^\s"'<>`]+/g) || [];
  const urls = matches
    .map((url) => url.replace(/[)\].,;:!?]+$/g, ''))
    .filter(Boolean);
  return (
    urls.find((url) => /(claude\.ai|anthropic\.com|oauth|authorize|login)/i.test(url)) ||
    urls[0] ||
    null
  );
}

function readPid() {
  const raw = readFile(pidPath).trim();
  if (!/^[0-9]+$/.test(raw)) return null;
  return Number(raw);
}

function pidAlive(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    return err && err.code === 'EPERM';
  }
}

function killProcessGroup(pid) {
  if (!pid) return;
  try {
    process.kill(-pid, 'SIGTERM');
  } catch (_) {}
  try {
    process.kill(pid, 'SIGTERM');
  } catch (_) {}
}

function claudeStatus() {
  const result = cp.spawnSync('claude', ['auth', 'status', '--json'], {
    env,
    encoding: 'utf8',
    timeout: 10000,
  });
  const stdout = String(result.stdout || '').trim();
  let parsed = null;
  try {
    parsed = stdout ? JSON.parse(stdout) : null;
  } catch (_) {}
  return {
    exit_status: typeof result.status === 'number' ? result.status : null,
    stdout,
    stderr: String(result.stderr || '').trim(),
    parsed,
  };
}

function pythonLoginWrapper() {
  return String.raw`
import os
import pty
import select
import sys
import time

log_path = sys.argv[1]
args = sys.argv[2:]
send_text = os.environ.get('CLAWMACDO_CLAUDE_LOGIN_STDIN', '')
send_after = float(os.environ.get('CLAWMACDO_CLAUDE_LOGIN_SEND_AFTER', '0.8'))

pid, fd = pty.fork()
if pid == 0:
    os.environ['TERM'] = 'dumb'
    os.environ['NO_COLOR'] = '1'
    os.environ['FORCE_COLOR'] = '0'
    os.execvp(args[0], args)

sent = False
started = time.time()
with open(log_path, 'ab', buffering=0) as log:
    while True:
        if send_text and not sent and time.time() - started >= send_after:
            os.write(fd, send_text.encode('utf-8'))
            sent = True
        readable, _, _ = select.select([fd], [], [], 0.2)
        if readable:
            try:
                data = os.read(fd, 4096)
            except OSError:
                break
            if not data:
                break
            log.write(data)
        try:
            done, _ = os.waitpid(pid, os.WNOHANG)
            if done:
                break
        except ChildProcessError:
            break
`;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main() {
  fs.mkdirSync(stateDir, { recursive: true, mode: 0o700 });

  if (!commandOk('command -v claude >/dev/null 2>&1')) {
    finish({
      ok: false,
      valid: false,
      status: 'claude_missing',
      login_url: null,
      error: 'Claude Code is not installed or not on PATH for the openclaw user.',
    });
    return;
  }

  const status = claudeStatus();
  if (status.parsed && status.parsed.loggedIn === true) {
    finish({
      ok: true,
      valid: true,
      status: 'authenticated',
      login_url: null,
      auth: status.parsed,
    });
    return;
  }

  const existingPid = readPid();
  const existingLog = readFile(logPath);
  const existingUrl = extractLoginUrl(existingLog) || readFile(urlPath).trim() || null;
  if (existingUrl && pidAlive(existingPid)) {
    finish({
      ok: true,
      valid: false,
      status: 'pending',
      login_url: existingUrl,
      pid: existingPid,
      log_path: logPath,
    });
    return;
  }

  killProcessGroup(existingPid);
  fs.rmSync(logPath, { force: true });
  fs.rmSync(urlPath, { force: true });

  if (!commandOk('command -v python3 >/dev/null 2>&1')) {
    finish({
      ok: false,
      valid: false,
      status: 'python_missing',
      login_url: null,
      error: 'python3 is required on the OpenClaw instance to run Claude Code login in a PTY.',
    });
    return;
  }

  const supportsAuthLogin = cp.spawnSync('claude', ['auth', 'login', '--help'], {
    env,
    encoding: 'utf8',
    timeout: 10000,
  }).status === 0;

  let loginArgs;
  let loginStdin = '';
  let method;
  if (supportsAuthLogin) {
    loginArgs = ['claude', 'auth', 'login', mode === 'console' ? '--console' : '--claudeai'];
    if (email) loginArgs.push('--email', email);
    if (useSso) loginArgs.push('--sso');
    method = 'auth-login';
  } else {
    loginArgs = ['claude'];
    loginStdin = '/login\n';
    method = 'slash-login';
  }

  const child = cp.spawn('python3', ['-c', pythonLoginWrapper(), logPath, ...loginArgs], {
    env: {
      ...env,
      CLAWMACDO_CLAUDE_LOGIN_STDIN: loginStdin,
      CLAWMACDO_CLAUDE_LOGIN_SEND_AFTER: '0.8',
    },
    detached: true,
    stdio: 'ignore',
  });
  fs.writeFileSync(pidPath, `${child.pid}\n`, { mode: 0o600 });
  child.unref();

  const deadline = Date.now() + waitMs;
  while (Date.now() < deadline) {
    await sleep(250);
    const log = readFile(logPath);
    const loginUrl = extractLoginUrl(log);
    if (loginUrl) {
      fs.writeFileSync(urlPath, `${loginUrl}\n`, { mode: 0o600 });
      finish({
        ok: true,
        valid: false,
        status: 'pending',
        login_url: loginUrl,
        pid: child.pid,
        method,
        log_path: logPath,
      });
      return;
    }
    if (!pidAlive(child.pid) && log.length > 0) break;
  }

  const finalLog = readFile(logPath);
  finish({
    ok: false,
    valid: false,
    status: 'login_url_not_found',
    login_url: null,
    pid: child.pid,
    method,
    log_path: logPath,
    error: 'Claude login started, but no login URL was captured before the wait budget expired.',
    output_tail: outputTail(finalLog),
  });
}

main().catch((err) => {
  finish({
    ok: false,
    valid: false,
    status: 'error',
    login_url: null,
    error: err && err.stack ? err.stack : String(err),
  });
});
NODE"#
        .to_string();

    for (needle, value) in [
        ("__OPENCLAW_HOME__", config::OPENCLAW_HOME.to_string()),
        ("__MODE_JSON__", js_string(mode)?),
        ("__EMAIL_JSON__", js_optional_string(email)?),
        ("__SSO_BOOL__", sso.to_string()),
        ("__WAIT_MS__", wait_ms.to_string()),
    ] {
        cmd = cmd.replace(needle, &value);
    }

    Ok(cmd)
}

fn build_status_cmd() -> String {
    let mut cmd = r#"export HOME="__OPENCLAW_HOME__" && \
         node <<'NODE'
const cp = require('child_process');
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '__OPENCLAW_HOME__';
const stateDir = path.join(home, '.openclaw', 'claude-auth');
const logPath = path.join(stateDir, 'login.log');
const pidPath = path.join(stateDir, 'login.pid');
const urlPath = path.join(stateDir, 'login-url.txt');
const env = {
  ...process.env,
  HOME: home,
  PATH: `${home}/.local/bin:${home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin`,
  TERM: 'dumb',
  NO_COLOR: '1',
  FORCE_COLOR: '0',
};

function finish(value) {
  console.log(JSON.stringify(value, null, 2));
}

function commandOk(command) {
  const result = cp.spawnSync('bash', ['-lc', command], {
    env,
    encoding: 'utf8',
    timeout: 10000,
  });
  return result.status === 0;
}

function stripAnsi(value) {
  return String(value || '').replace(/\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])/g, '');
}

function readFile(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch (_) {
    return '';
  }
}

function extractLoginUrl(value) {
  const clean = stripAnsi(value);
  const matches = clean.match(/https?:\/\/[^\s"'<>`]+/g) || [];
  const urls = matches
    .map((url) => url.replace(/[)\].,;:!?]+$/g, ''))
    .filter(Boolean);
  return (
    urls.find((url) => /(claude\.ai|anthropic\.com|oauth|authorize|login)/i.test(url)) ||
    urls[0] ||
    null
  );
}

function readPid() {
  const raw = readFile(pidPath).trim();
  if (!/^[0-9]+$/.test(raw)) return null;
  return Number(raw);
}

function pidAlive(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    return err && err.code === 'EPERM';
  }
}

function claudeStatus() {
  const result = cp.spawnSync('claude', ['auth', 'status', '--json'], {
    env,
    encoding: 'utf8',
    timeout: 10000,
  });
  const stdout = String(result.stdout || '').trim();
  let parsed = null;
  try {
    parsed = stdout ? JSON.parse(stdout) : null;
  } catch (_) {}
  return {
    exit_status: typeof result.status === 'number' ? result.status : null,
    stdout,
    stderr: String(result.stderr || '').trim(),
    parsed,
  };
}

if (!commandOk('command -v claude >/dev/null 2>&1')) {
  finish({
    ok: false,
    valid: false,
    status: 'claude_missing',
    login_url: null,
    error: 'Claude Code is not installed or not on PATH for the openclaw user.',
  });
} else {
  const auth = claudeStatus();
  const valid = !!(auth.parsed && auth.parsed.loggedIn === true);
  const pid = readPid();
  const loginUrl = extractLoginUrl(readFile(logPath)) || readFile(urlPath).trim() || null;
  const loginProcessActive = pidAlive(pid);
  finish({
    ok: true,
    valid,
    status: valid ? 'authenticated' : (loginProcessActive ? 'pending' : 'not_authenticated'),
    login_url: loginUrl,
    login_process_active: loginProcessActive,
    pid,
    auth: auth.parsed,
    auth_exit_status: auth.exit_status,
    auth_stderr: auth.stderr || null,
    log_path: logPath,
    checked_at: new Date().toISOString(),
  });
}
NODE"#
        .to_string();

    cmd = cmd.replace("__OPENCLAW_HOME__", config::OPENCLAW_HOME);
    cmd
}

pub async fn start(params: ClaudeAuthStartParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let mode = clean_mode(&params.mode)?;
    let email = clean_email(params.email)?;
    let (ip, key, provider) = find_deploy_record(&instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let cmd = build_start_cmd(&mode, email.as_deref(), params.sso, params.wait_secs)?;
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let mut value = remote_json_value(&output, "Claude auth start")?;
    insert_context(&mut value, &instance, &ip);

    if params.json {
        return print_json(&value);
    }

    if value_bool(&value, "valid") {
        println!("Claude Code is already authenticated on {instance}.");
        return Ok(());
    }

    if let Some(url) = value_str(&value, "login_url") {
        println!("{url}");
        return Ok(());
    }

    let error = value_str(&value, "error").unwrap_or("Claude login URL was not captured.");
    bail!("{error}");
}

pub async fn status(params: ClaudeAuthStatusParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let (ip, key, provider) = find_deploy_record(&instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &build_status_cmd(), ssh_user).await?;
    let mut value = remote_json_value(&output, "Claude auth status")?;
    insert_context(&mut value, &instance, &ip);

    if params.json {
        return print_json(&value);
    }

    let status = value_str(&value, "status").unwrap_or("unknown");
    if value_bool(&value, "valid") {
        println!("Claude Code auth status: authenticated");
    } else {
        println!("Claude Code auth status: {status}");
        if let Some(url) = value_str(&value, "login_url") {
            println!("Login URL: {url}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_instance_rejects_empty_and_control_chars() {
        assert!(clean_instance("").is_err());
        assert!(clean_instance("openclaw\nbad").is_err());
        assert_eq!(clean_instance(" openclaw-a1 ").unwrap(), "openclaw-a1");
    }

    #[test]
    fn clean_mode_accepts_supported_modes() {
        assert_eq!(clean_mode("claudeai").unwrap(), "claudeai");
        assert_eq!(clean_mode("console").unwrap(), "console");
        assert!(clean_mode("bad").is_err());
    }

    #[test]
    fn build_start_command_contains_json_contract_and_auth_login() {
        let cmd = build_start_cmd("claudeai", Some("user@example.com"), true, 20).unwrap();
        assert!(cmd.contains("claude"));
        assert!(cmd.contains("auth"));
        assert!(cmd.contains("login"));
        assert!(cmd.contains("login_url"));
        assert!(cmd.contains("\"user@example.com\""));
    }

    #[test]
    fn build_status_command_uses_claude_auth_status_json() {
        let cmd = build_status_cmd();
        assert!(cmd.contains("auth"));
        assert!(cmd.contains("status"));
        assert!(cmd.contains("--json"));
        assert!(cmd.contains("valid"));
    }
}
