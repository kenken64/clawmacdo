use anyhow::{bail, Context, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_with_user_multi_async, ssh_root_as_async,
};
use std::path::PathBuf;

const DEFAULT_CHAT_BASE_URL: &str = "http://127.0.0.1:18789/v1";

pub struct RemotionAvatarParams {
    pub instance: String,
    pub name: String,
    pub app_dir: String,
    pub port: u16,
    pub chat_model: String,
    pub openai_api_key: Option<String>,
    pub voice_gender: String,
    pub avatar_glb: Option<PathBuf>,
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

fn clean_app_dir(value: &str) -> Result<String> {
    let app_dir = clean_required("app-dir", value, 512)?;
    let workspace = format!("{}/.openclaw/workspace/", config::OPENCLAW_HOME);
    if !app_dir.starts_with(&workspace) {
        bail!("--app-dir must be under {workspace}");
    }
    if app_dir.contains("/../") || app_dir.ends_with("/..") {
        bail!("--app-dir cannot contain '..' path segments.");
    }
    Ok(app_dir)
}

fn clean_avatar_glb_path(value: Option<PathBuf>) -> Result<Option<PathBuf>> {
    let Some(path) = value else {
        return Ok(None);
    };
    if !path.exists() {
        bail!("--avatar-glb file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("--avatar-glb must point to a file: {}", path.display());
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("--avatar-glb filename must be valid UTF-8."))?;
    let lower = file_name.to_ascii_lowercase();
    if lower != "avatar.glb" && !lower.ends_with("_avatar.glb") {
        bail!("--avatar-glb filename must be avatar.glb or end with _avatar.glb.");
    }

    Ok(Some(path.canonicalize().with_context(|| {
        format!("Failed to resolve --avatar-glb path {}", path.display())
    })?))
}

fn install_cloudflared_cmd() -> String {
    r#"set -e
if command -v cloudflared >/dev/null 2>&1; then
  echo "cloudflared: $(cloudflared --version | head -1)"
  loginctl enable-linger openclaw 2>/dev/null || true
  exit 0
fi

ARCH=$(dpkg --print-architecture 2>/dev/null || uname -m)
case "$ARCH" in
  amd64|x86_64) CF_ARCH=amd64 ;;
  arm64|aarch64) CF_ARCH=arm64 ;;
  *) echo "Unsupported architecture for cloudflared: $ARCH"; exit 1 ;;
esac

TMP="/tmp/cloudflared-linux-${CF_ARCH}.deb"
curl -fsSL -o "$TMP" "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-${CF_ARCH}.deb"
dpkg -i "$TMP" || apt-get install -y -f
loginctl enable-linger openclaw 2>/dev/null || true
echo "cloudflared: $(cloudflared --version | head -1)"
"#
    .to_string()
}

fn configure_app_cmd(params: &RemotionAvatarParams, avatar_upload_tmp: Option<&str>) -> String {
    let app_dir = shell_escape(&params.app_dir);
    let name = shell_escape(&params.name);
    let chat_model = shell_escape(&params.chat_model);
    let openai_api_key = shell_escape(params.openai_api_key.as_deref().unwrap_or(""));
    let voice_gender = shell_escape(&params.voice_gender);
    let tts_voice = shell_escape(tts_voice_for_gender(&params.voice_gender));
    let avatar_upload_tmp = shell_escape(avatar_upload_tmp.unwrap_or(""));
    let home = config::OPENCLAW_HOME;
    let port = params.port;

    format!(
        r#"set -e
export HOME="{home}"
export PATH="{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"
export APP_DIR={app_dir}
export AVATAR_NAME={name}
export REMOTION_PORT="{port}"
export CHAT_BASE_URL="{chat_base_url}"
export CHAT_MODEL={chat_model}
export REQUESTED_OPENAI_API_KEY={openai_api_key}
export VOICE_GENDER={voice_gender}
export TTS_VOICE={tts_voice}
export AVATAR_UPLOAD_TMP={avatar_upload_tmp}

CONFIG="{home}/.openclaw/openclaw.json"
CHAT_API_KEY=$(node -e "const fs=require('fs');const p=process.env.CONFIG||'{home}/.openclaw/openclaw.json';try{{const cfg=JSON.parse(fs.readFileSync(p,'utf8'));process.stdout.write((cfg.gateway&&cfg.gateway.auth&&cfg.gateway.auth.token)||'')}}catch(e){{process.exit(1)}}")
if [ -z "$CHAT_API_KEY" ]; then
  echo "OpenClaw gateway token not found in $CONFIG"
  exit 1
fi

if [ ! -d "$APP_DIR" ]; then
  echo "Remotion app directory not found: $APP_DIR"
  exit 1
fi

cd "$APP_DIR"
OPENAI_API_KEY_VALUE="$REQUESTED_OPENAI_API_KEY"
OPENAI_API_KEY_SOURCE="provided"
if [ -z "$OPENAI_API_KEY_VALUE" ] && [ -f .env ]; then
  OPENAI_API_KEY_VALUE=$(node -e "const fs=require('fs');const text=fs.readFileSync('.env','utf8');const line=text.split(/\r?\n/).find(l=>l.startsWith('OPENAI_API_KEY='));process.stdout.write(line ? line.slice('OPENAI_API_KEY='.length).trim() : '')" 2>/dev/null || true)
  if [ -n "$OPENAI_API_KEY_VALUE" ]; then
    OPENAI_API_KEY_SOURCE="existing_env"
  fi
fi
if [ -z "$OPENAI_API_KEY_VALUE" ]; then
  OPENAI_API_KEY_VALUE="$CHAT_API_KEY"
  OPENAI_API_KEY_SOURCE="gateway_token_fallback"
fi

umask 077
cat > .env <<ENVEOF
CHAT_BASE_URL=$CHAT_BASE_URL
CHAT_API_KEY=$CHAT_API_KEY
CHAT_MODEL=$CHAT_MODEL
VITE_AVATAR_NAME=$AVATAR_NAME
OPENAI_API_KEY=$OPENAI_API_KEY_VALUE
VOICE_GENDER=$VOICE_GENDER
TTS_VOICE=$TTS_VOICE
ENVEOF
chmod 600 .env
echo "env: $APP_DIR/.env"
echo "openai_api_key_source=$OPENAI_API_KEY_SOURCE"
echo "voice_gender=$VOICE_GENDER"
echo "tts_voice=$TTS_VOICE"

if [ -n "$AVATAR_UPLOAD_TMP" ]; then
  if [ ! -f "$AVATAR_UPLOAD_TMP" ]; then
    echo "Avatar upload file not found: $AVATAR_UPLOAD_TMP"
    exit 1
  fi
  mkdir -p "$APP_DIR/public"
  install -m 0644 "$AVATAR_UPLOAD_TMP" "$APP_DIR/public/avatar.glb"
  rm -f "$AVATAR_UPLOAD_TMP"
  echo "avatar_glb=$APP_DIR/public/avatar.glb"
fi

if command -v claude >/dev/null 2>&1; then
  timeout 180 claude -p "In this project, find all user-facing occurrences of kenken64 and replace them with $AVATAR_NAME. Keep behavior unchanged." \
    --dangerously-skip-permissions </dev/null >/tmp/clawmacdo-remotion-claude.log 2>&1 || true
fi

node <<'NODE'
const fs = require('fs');
const path = require('path');
const root = process.env.APP_DIR;
const replacement = process.env.AVATAR_NAME;
const skipDirs = new Set(['.git', 'node_modules', 'dist', 'build', '.next', 'out', '.turbo', '.cache']);
const allowedExts = new Set(['.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs', '.json', '.md', '.mdx', '.css', '.scss', '.html', '.txt', '.env', '.yml', '.yaml']);
let changed = 0;

function walk(dir) {{
  for (const item of fs.readdirSync(dir, {{ withFileTypes: true }})) {{
    if (skipDirs.has(item.name)) continue;
    const itemPath = path.join(dir, item.name);
    if (item.isDirectory()) {{
      walk(itemPath);
      continue;
    }}
    if (!item.isFile()) continue;
    const ext = path.extname(item.name).toLowerCase();
    if (!allowedExts.has(ext) && !item.name.startsWith('.env')) continue;
    let text;
    try {{
      text = fs.readFileSync(itemPath, 'utf8');
    }} catch (_) {{
      continue;
    }}
    if (!text.includes('kenken64')) continue;
    fs.writeFileSync(itemPath, text.split('kenken64').join(replacement));
    changed += 1;
  }}
}}

walk(root);
console.log('name_replacement_files=' + changed);
NODE

if [ -f package.json ]; then
  if command -v pnpm >/dev/null 2>&1 && [ -f pnpm-lock.yaml ]; then
    pnpm install --frozen-lockfile || pnpm install
  elif command -v npm >/dev/null 2>&1; then
    if [ -f package-lock.json ]; then npm ci || npm install; else npm install; fi
  fi
fi

mkdir -p "$HOME/.local/bin" "$HOME/.config/systemd/user"

cat > "$HOME/.local/bin/remotion-avatar-start" <<SHEOF
#!/usr/bin/env bash
set -e
export HOME="{home}"
export PATH="{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:\$PATH"
cd "$APP_DIR"
export HOST="\${{HOST:-0.0.0.0}}"

has_script() {{
  node -e "const p=require('./package.json'); process.exit(p.scripts && p.scripts[process.argv[1]] ? 0 : 1)" "\$1" >/dev/null 2>&1
}}

if [ -f package.json ] && command -v node >/dev/null 2>&1; then
  if has_script dev; then
    if command -v pnpm >/dev/null 2>&1 && [ -f pnpm-lock.yaml ]; then
      exec pnpm run dev
    fi
    exec npm run dev
  fi
  if has_script start; then
    if command -v pnpm >/dev/null 2>&1 && [ -f pnpm-lock.yaml ]; then
      exec pnpm run start
    fi
    exec npm run start
  fi
fi

if command -v pnpm >/dev/null 2>&1; then
  exec pnpm exec remotion studio --host 0.0.0.0 --port "{port}"
fi
exec npx remotion studio --host 0.0.0.0 --port "{port}"
SHEOF
chmod +x "$HOME/.local/bin/remotion-avatar-start"

cat > "$HOME/.local/bin/remotion-avatar-tunnel" <<SHEOF
#!/usr/bin/env bash
set -e
export PATH="/usr/local/bin:/usr/bin:/bin:\$PATH"
LOG_DIR="{home}/.local/state/remotion-avatar"
LOG_FILE="\$LOG_DIR/cloudflared.log"
mkdir -p "\$LOG_DIR"
exec >>"\$LOG_FILE" 2>&1
exec cloudflared tunnel --no-autoupdate --url "http://127.0.0.1:{port}"
SHEOF
chmod +x "$HOME/.local/bin/remotion-avatar-tunnel"

cat > "$HOME/.config/systemd/user/remotion-avatar.service" <<UNITEOF
[Unit]
Description=Remotion 3D AI Avatar
After=network-online.target

[Service]
Type=simple
WorkingDirectory=$APP_DIR
ExecStart={home}/.local/bin/remotion-avatar-start
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
UNITEOF

cat > "$HOME/.config/systemd/user/remotion-avatar-tunnel.service" <<UNITEOF
[Unit]
Description=Cloudflare Quick Tunnel for Remotion 3D AI Avatar
After=network-online.target remotion-avatar.service
Wants=remotion-avatar.service

[Service]
Type=simple
ExecStart={home}/.local/bin/remotion-avatar-tunnel
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
UNITEOF

export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
LOG_DIR="$HOME/.local/state/remotion-avatar"
LOG_FILE="$LOG_DIR/cloudflared.log"
mkdir -p "$LOG_DIR"
: > "$LOG_FILE"
systemctl --user daemon-reload
systemctl --user enable remotion-avatar.service remotion-avatar-tunnel.service >/dev/null 2>&1 || true
systemctl --user restart remotion-avatar.service
sleep 4
systemctl --user restart remotion-avatar-tunnel.service

URL=""
for _ in $(seq 1 40); do
  sleep 2
  URL=$(grep -Eo 'https://[-a-zA-Z0-9.]+\.trycloudflare\.com' "$LOG_FILE" 2>/dev/null | tail -1 || true)
  if [ -n "$URL" ]; then break; fi
done

if [ -z "$URL" ]; then
  echo "CLOUDFLARED_URL="
  tail -80 "$LOG_FILE" || true
  exit 1
fi

echo "CLOUDFLARED_URL=$URL"
echo "remotion_service=$(systemctl --user is-active remotion-avatar.service 2>/dev/null || true)"
echo "tunnel_service=$(systemctl --user is-active remotion-avatar-tunnel.service 2>/dev/null || true)"
"#,
        home = home,
        app_dir = app_dir,
        name = name,
        port = port,
        chat_base_url = DEFAULT_CHAT_BASE_URL,
        chat_model = chat_model,
        openai_api_key = openai_api_key,
        voice_gender = voice_gender,
        tts_voice = tts_voice,
        avatar_upload_tmp = avatar_upload_tmp,
    )
}

pub async fn setup(params: RemotionAvatarParams) -> Result<()> {
    let avatar_glb = clean_avatar_glb_path(params.avatar_glb)?;
    let params = RemotionAvatarParams {
        instance: clean_required("instance", &params.instance, 255)?,
        name: clean_required("name", &params.name, 120)?,
        app_dir: clean_app_dir(&params.app_dir)?,
        port: params.port,
        chat_model: clean_required("chat-model", &params.chat_model, 80)?,
        openai_api_key: clean_optional_secret("openai-api-key", params.openai_api_key, 4096)?,
        voice_gender: clean_voice_gender(&params.voice_gender)?,
        avatar_glb: None,
        json: params.json,
    };

    let (ip, key, provider) = find_deploy_record(&params.instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let total_steps = if avatar_glb.is_some() { 3 } else { 2 };

    if !params.json {
        println!("Setting up Remotion avatar app on {ip}...");
        println!("[1/{total_steps}] Installing cloudflared if needed...");
    }
    let install_out = ssh_root_as_async(&ip, &key, &install_cloudflared_cmd(), ssh_user).await?;
    if !params.json && !install_out.trim().is_empty() {
        println!("  {}", install_out.trim());
    }

    let avatar_upload_tmp = if let Some(avatar_glb) = avatar_glb {
        let remote_tmp = format!(
            "/tmp/clawmacdo-remotion-avatar-{}.glb",
            uuid::Uuid::new_v4()
        );
        if !params.json {
            println!("[2/{total_steps}] Uploading avatar GLB...");
        }
        let scp_ip = ip.clone();
        let scp_key = key.clone();
        let scp_user = ssh_user.to_string();
        let remote_tmp_for_upload = remote_tmp.clone();
        tokio::task::spawn_blocking(move || {
            clawmacdo_ssh::scp_upload_as(
                &scp_ip,
                &scp_key,
                &avatar_glb,
                &remote_tmp_for_upload,
                &scp_user,
            )
        })
        .await
        .context("avatar upload task failed")??;

        let remote_tmp_escaped = shell_escape(&remote_tmp);
        ssh_root_as_async(
            &ip,
            &key,
            &format!(
                "chown openclaw:openclaw {remote_tmp_escaped} && chmod 0644 {remote_tmp_escaped}"
            ),
            ssh_user,
        )
        .await?;
        if !params.json {
            println!("  uploaded as public/avatar.glb");
        }
        Some(remote_tmp)
    } else {
        None
    };

    if !params.json {
        println!("[{total_steps}/{total_steps}] Configuring app, services, and Quick Tunnel...");
    }
    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![configure_app_cmd(&params, avatar_upload_tmp.as_deref())],
        ssh_user,
    )
    .await?;

    let out = outputs.first().map(|s| s.trim()).unwrap_or("");
    let tunnel_url = out
        .lines()
        .find_map(|line| line.strip_prefix("CLOUDFLARED_URL="))
        .unwrap_or("");

    let remotion_service = out
        .lines()
        .find_map(|line| line.strip_prefix("remotion_service="))
        .unwrap_or("");
    let tunnel_service = out
        .lines()
        .find_map(|line| line.strip_prefix("tunnel_service="))
        .unwrap_or("");

    if params.json {
        if tunnel_url.is_empty() {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ok": false,
                    "error": "Cloudflared URL was not returned. Check remotion-avatar-tunnel.service logs.",
                    "remotion_url": null,
                    "remotion_service": remotion_service,
                    "tunnel_service": tunnel_service
                }))?
            );
            bail!("Cloudflared URL was not returned. Check remotion-avatar-tunnel.service logs.");
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "remotion_url": tunnel_url,
                "remotion_service": remotion_service,
                "tunnel_service": tunnel_service
            }))?
        );
        return Ok(());
    }

    for line in out.lines() {
        if line.starts_with("env:")
            || line.starts_with("name_replacement_files=")
            || line.starts_with("openai_api_key_source=")
            || line.starts_with("voice_gender=")
            || line.starts_with("tts_voice=")
            || line.starts_with("avatar_glb=")
            || line.starts_with("remotion_service=")
            || line.starts_with("tunnel_service=")
        {
            println!("  {line}");
        }
    }

    if tunnel_url.is_empty() {
        bail!("Cloudflared URL was not returned. Check remotion-avatar-tunnel.service logs.");
    }

    println!();
    println!("Remotion avatar app is available at:");
    println!("  {tunnel_url}");

    Ok(())
}
