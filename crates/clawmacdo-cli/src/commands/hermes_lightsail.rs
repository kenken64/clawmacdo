use anyhow::{bail, Context, Result};
use base64::Engine;
use chrono::Utc;
use clawmacdo_cloud::cloud_provider::CreateInstanceParams;
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_cloud::CloudProvider;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_db as db;
use clawmacdo_ssh as ssh;
use clawmacdo_ui::progress;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub type Db = Arc<Mutex<rusqlite::Connection>>;

pub const TOTAL_STEPS: i32 = 8;
pub const DEFAULT_BEDROCK_REGION: &str = "ap-southeast-1";
pub const DEFAULT_BEDROCK_MODEL: &str = "amazon.nova-pro-v1:0";
pub const DEFAULT_OPENROUTER_MODEL: &str = "openai/gpt-6-sol";
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProvider {
    OpenRouter,
    Bedrock,
}

pub struct HermesLightsailParams {
    pub deploy_id: Option<String>,
    pub customer_email: String,
    pub name: Option<String>,
    pub aws_access_key_id: String,
    pub aws_secret_access_key: String,
    pub aws_region: String,
    pub size: Option<String>,
    pub env_file: Option<PathBuf>,
    pub env_content: Option<String>,
    pub env_vars: Vec<String>,
    pub ai_provider: String,
    pub openrouter_api_key: Option<String>,
    pub openrouter_model: String,
    pub bedrock_api_key: Option<String>,
    pub bedrock_region: String,
    pub bedrock_model: String,
    pub install_opencode: bool,
    pub telegram_bot_token: Option<String>,
    pub telegram_allowed_users: Option<String>,
    pub telegram_home_channel: Option<String>,
    pub telegram_pair_code: Option<String>,
    pub dashboard: bool,
    pub cloudflare_tunnel: bool,
    pub dashboard_password: Option<String>,
    pub dry_run: bool,
    pub json: bool,
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
    pub db: Option<Db>,
}

fn record_step_start(db: &Option<Db>, deploy_id: &str, step: i32, label: &str) {
    db::record_step_start(db, deploy_id, step, TOTAL_STEPS, label);
}

fn record_step_complete(db: &Option<Db>, deploy_id: &str, step: i32) {
    db::record_step_complete(db, deploy_id, step);
}

fn record_step_failed(db: &Option<Db>, deploy_id: &str, step: i32, err: &str) {
    db::record_step_failed(db, deploy_id, step, err);
}

fn record_step_skipped(db: &Option<Db>, deploy_id: &str, step: i32) {
    db::record_step_skipped(db, deploy_id, step);
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn validate_env_assignment(value: &str) -> Result<()> {
    let Some((key, _)) = value.split_once('=') else {
        bail!("--env values must use KEY=VALUE syntax");
    };
    if key.is_empty() {
        bail!("--env key cannot be empty");
    }
    let mut chars = key.chars();
    let first = chars.next().unwrap_or_default();
    if !(first == '_' || first.is_ascii_alphabetic())
        || !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        bail!("Invalid env key '{key}'. Use shell-style names like OPENAI_API_KEY.");
    }
    Ok(())
}

fn append_env_assignment(out: &mut String, key: &str, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(key);
    out.push('=');
    out.push_str(trimmed);
    out.push('\n');
}

fn normalize_csv(value: &str) -> Option<String> {
    let normalized = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn first_csv_value(value: &str) -> Option<String> {
    value
        .split(',')
        .map(str::trim)
        .find(|part| !part.is_empty())
        .map(str::to_string)
}

fn normalized_bedrock_region(params: &HermesLightsailParams) -> String {
    let region = params.bedrock_region.trim();
    if region.is_empty() {
        DEFAULT_BEDROCK_REGION.to_string()
    } else {
        region.to_string()
    }
}

fn normalized_bedrock_model(params: &HermesLightsailParams) -> String {
    let model = params.bedrock_model.trim();
    if model.is_empty() {
        DEFAULT_BEDROCK_MODEL.to_string()
    } else {
        model.to_string()
    }
}

fn normalized_openrouter_model(params: &HermesLightsailParams) -> String {
    let model = params.openrouter_model.trim();
    if model.is_empty() {
        DEFAULT_OPENROUTER_MODEL.to_string()
    } else {
        model.to_string()
    }
}

fn has_value(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| !value.is_empty())
}

/// Validate a Telegram bot token of the form `<bot_id>:<secret>`.
fn validate_telegram_bot_token(token: &str) -> Result<()> {
    let token = token.trim();
    let Some((bot_id, secret)) = token.split_once(':') else {
        bail!("Invalid Telegram bot token: expected <bot_id>:<secret> from @BotFather.");
    };
    if bot_id.is_empty() || !bot_id.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("Invalid Telegram bot token: the bot ID before ':' must be numeric.");
    }
    if secret.is_empty()
        || !secret
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("Invalid Telegram bot token: the secret after ':' contains unexpected characters.");
    }
    Ok(())
}

pub(super) fn validate_telegram_pair_code(code: &str) -> Result<()> {
    let code = code.trim();
    if !(4..=32).contains(&code.len()) || !code.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        bail!("Invalid Telegram pairing code '{code}'. Expected the alphanumeric code shown by the bot.");
    }
    Ok(())
}

enum TelegramVerification {
    Verified(String),
    Unverified(String),
}

/// Call Telegram's `getMe` endpoint to confirm the bot token is live.
///
/// An invalid token is a hard error. Network failures return `Unverified` so
/// provisioning can still proceed and be verified later.
async fn verify_telegram_bot_token(token: &str) -> Result<TelegramVerification> {
    validate_telegram_bot_token(token)?;
    let url = format!("https://api.telegram.org/bot{}/getMe", token.trim());
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(err) => return Ok(TelegramVerification::Unverified(err.to_string())),
    };
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(err) => return Ok(TelegramVerification::Unverified(err.to_string())),
    };
    let payload: serde_json::Value = match response.json().await {
        Ok(payload) => payload,
        Err(err) => return Ok(TelegramVerification::Unverified(err.to_string())),
    };
    if payload.get("ok").and_then(|value| value.as_bool()) == Some(true) {
        let username = payload
            .pointer("/result/username")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        Ok(TelegramVerification::Verified(username))
    } else {
        let description = payload
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or("Telegram rejected the bot token");
        bail!("Telegram bot token verification failed: {description}");
    }
}

/// Resolve which AI provider to configure.
///
/// An explicit `--ai-provider` wins. Otherwise OpenRouter is preferred when
/// an OpenRouter key is supplied, falling back to Bedrock for backwards
/// compatibility with existing `--bedrock-api-key` deployments.
fn resolve_ai_provider(params: &HermesLightsailParams) -> Result<AiProvider> {
    let requested = params.ai_provider.trim().to_ascii_lowercase();
    let has_openrouter = has_value(params.openrouter_api_key.as_deref());
    let has_bedrock = has_value(params.bedrock_api_key.as_deref());

    match requested.as_str() {
        "openrouter" => {
            if !has_openrouter {
                bail!(
                    "OpenRouter provider selected but no API key was supplied. \
                     Pass --openrouter-api-key or set OPENROUTER_API_KEY."
                );
            }
            Ok(AiProvider::OpenRouter)
        }
        "bedrock" | "aws-bedrock" => {
            if !has_bedrock {
                bail!(
                    "Bedrock provider selected but no API key was supplied. \
                     Pass --bedrock-api-key or set AWS_BEARER_TOKEN_BEDROCK."
                );
            }
            Ok(AiProvider::Bedrock)
        }
        "" | "auto" => {
            if has_openrouter {
                Ok(AiProvider::OpenRouter)
            } else if has_bedrock {
                Ok(AiProvider::Bedrock)
            } else {
                bail!(
                    "AI provider credentials required. Set OPENROUTER_API_KEY \
                     (recommended) or AWS_BEARER_TOKEN_BEDROCK."
                );
            }
        }
        other => bail!("Unsupported --ai-provider '{other}'. Use openrouter or bedrock."),
    }
}

/// Validate web requests before a deployment job or cloud resource is created.
pub(super) fn validate_configuration(params: &HermesLightsailParams) -> Result<()> {
    resolve_ai_provider(params)?;
    dashboard_tunnel(params)?;
    if let Some(token) = params
        .telegram_bot_token
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        validate_telegram_bot_token(token)?;
    }
    if let Some(code) = params
        .telegram_pair_code
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        if !has_value(params.telegram_bot_token.as_deref()) {
            bail!("A Telegram bot token is required to approve a pairing code.");
        }
        validate_telegram_pair_code(code)?;
    }
    Ok(())
}

struct DashboardTunnel {
    password: String,
}

fn dashboard_tunnel(params: &HermesLightsailParams) -> Result<Option<DashboardTunnel>> {
    if !params.cloudflare_tunnel {
        return Ok(None);
    }
    let password = params.dashboard_password.as_deref().unwrap_or("");
    if password.trim().is_empty()
        || password.chars().count() < 12
        || password.chars().any(char::is_control)
    {
        bail!("Quick Tunnel requires a dashboard password of at least 12 characters and no control characters.");
    }
    Ok(Some(DashboardTunnel {
        password: password.to_string(),
    }))
}

fn parse_quick_tunnel_url(value: &str) -> Result<String> {
    let value = value.trim();
    let label = value
        .strip_prefix("https://")
        .and_then(|url| url.strip_suffix(".trycloudflare.com"));
    if !label.is_some_and(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }) {
        bail!("The Hermes host did not return a valid Quick Tunnel URL.");
    }
    Ok(value.to_string())
}

async fn read_dashboard_url(ip: &str, key: &std::path::Path) -> Result<String> {
    let ip = ip.to_string();
    let key = key.to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        ssh::exec_as(
            &ip,
            &key,
            "cat /var/lib/hermes-cloudflared/dashboard-url",
            "ubuntu",
        )
    })
    .await??;
    parse_quick_tunnel_url(&output)
}

fn bedrock_mantle_base_url(region: &str) -> String {
    format!("https://bedrock-mantle.{region}.api.aws/v1")
}

fn yaml_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn build_bedrock_config_content(region: &str, model: &str) -> String {
    let base_url = bedrock_mantle_base_url(region);
    format!(
        "model:\n  default: {}\n  provider: custom\n  base_url: {}\nbedrock:\n  region: {}\nterminal:\n  backend: local\n",
        yaml_single_quote(model),
        yaml_single_quote(&base_url),
        yaml_single_quote(region),
    )
}

fn build_openrouter_config_content(model: &str) -> String {
    format!(
        "model:\n  default: {}\n  provider: openrouter\n  base_url: {}\nterminal:\n  backend: local\n",
        yaml_single_quote(model),
        yaml_single_quote(OPENROUTER_BASE_URL),
    )
}

/// OpenCode V2 global config that routes the CLI through OpenRouter using the
/// same model as Hermes.
fn build_opencode_config_content(model: &str) -> String {
    let payload = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "model": format!("openrouter/{model}"),
        "providers": {
            "openrouter": {
                "env": ["OPENROUTER_API_KEY"],
                "settings": {
                    "apiKey": "{file:~/.config/opencode/openrouter-api-key}",
                },
            },
        },
    });
    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
}

fn build_aws_cli_config_content(region: &str) -> String {
    format!("[default]\nregion = {region}\noutput = json\n")
}

fn build_env_content(params: &HermesLightsailParams) -> Result<String> {
    let mut chunks = Vec::new();

    if let Some(path) = params.env_file.as_deref() {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read env file {}", path.display()))?;
        chunks.push(contents.trim_end().to_string());
    }

    if let Some(contents) = params.env_content.as_deref() {
        let trimmed = contents.trim_end();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }
    }

    for item in &params.env_vars {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        validate_env_assignment(trimmed)?;
        chunks.push(trimmed.to_string());
    }

    let mut env = chunks
        .into_iter()
        .filter(|chunk| !chunk.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !env.is_empty() && !env.ends_with('\n') {
        env.push('\n');
    }
    match resolve_ai_provider(params)? {
        AiProvider::OpenRouter => {
            if let Some(api_key) = params.openrouter_api_key.as_deref() {
                append_env_assignment(&mut env, "OPENROUTER_API_KEY", api_key);
            }
        }
        AiProvider::Bedrock => {
            if let Some(api_key) = params.bedrock_api_key.as_deref() {
                let region = normalized_bedrock_region(params);
                let base_url = bedrock_mantle_base_url(&region);
                append_env_assignment(&mut env, "AWS_BEARER_TOKEN_BEDROCK", api_key);
                append_env_assignment(&mut env, "OPENAI_API_KEY", api_key);
                append_env_assignment(&mut env, "OPENAI_BASE_URL", &base_url);
                append_env_assignment(&mut env, "AWS_REGION", &region);
                append_env_assignment(&mut env, "AWS_DEFAULT_REGION", &region);
            }
        }
    }
    if let Some(token) = params.telegram_bot_token.as_deref() {
        append_env_assignment(&mut env, "TELEGRAM_BOT_TOKEN", token);
    }
    let allowed_users = params
        .telegram_allowed_users
        .as_deref()
        .and_then(normalize_csv);
    if let Some(users) = allowed_users.as_deref() {
        append_env_assignment(&mut env, "TELEGRAM_ALLOWED_USERS", users);
    }
    let home_channel = params
        .telegram_home_channel
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| allowed_users.as_deref().and_then(first_csv_value));
    if let Some(channel) = home_channel.as_deref() {
        append_env_assignment(&mut env, "TELEGRAM_HOME_CHANNEL", channel);
    }
    append_env_assignment(&mut env, "TERMINAL_ENV", "local");
    Ok(env)
}

fn build_opencode_install_block(api_key: Option<&str>, config_content: &str) -> String {
    let Some(api_key) = api_key.map(str::trim).filter(|value| !value.is_empty()) else {
        return "# OpenCode CLI install skipped (no OpenRouter API key)\n".to_string();
    };
    let key_export_q = sh_quote(&format!("export OPENROUTER_API_KEY={}", sh_quote(api_key)));
    let key_q = sh_quote(api_key);
    let config_b64 = base64::engine::general_purpose::STANDARD.encode(config_content.as_bytes());
    format!(
        r#"echo "[clawmacdo] Installing OpenCode CLI v2 for ubuntu user"
OPENCODE_CONFIG_B64='{config_b64}'
OC_HOME=/home/ubuntu
mkdir -p "$OC_HOME/.config/opencode"
printf '%s' {key_q} > "$OC_HOME/.config/opencode/openrouter-api-key"
chmod 600 "$OC_HOME/.config/opencode/openrouter-api-key"
if [ -n "$OPENCODE_CONFIG_B64" ]; then
  printf '%s' "$OPENCODE_CONFIG_B64" | base64 -d > "$OC_HOME/.config/opencode/opencode.json"
fi
chown -R ubuntu:ubuntu "$OC_HOME/.config"

if runuser -u ubuntu -- env HOME="$OC_HOME" bash -o pipefail -c 'curl -fsSL https://opencode.ai/v2/install | bash -s -- --no-modify-path'; then
  echo "[clawmacdo] OpenCode CLI installed"
else
  echo "[clawmacdo] OpenCode CLI v2 install failed" >&2
  exit 1
fi

test -x "$OC_HOME/.opencode/bin/opencode"
ln -sf "$OC_HOME/.opencode/bin/opencode" /usr/local/bin/opencode
runuser -u ubuntu -- env HOME="$OC_HOME" "$OC_HOME/.opencode/bin/opencode" --version

if ! grep -q '\.opencode/bin' "$OC_HOME/.bashrc" 2>/dev/null; then
  printf '\nexport PATH="$HOME/.opencode/bin:$PATH"\n' >> "$OC_HOME/.bashrc"
fi
if ! grep -q '^export OPENROUTER_API_KEY=' "$OC_HOME/.bashrc" 2>/dev/null; then
  printf '%s\n' {key_export_q} >> "$OC_HOME/.bashrc"
fi
chown ubuntu:ubuntu "$OC_HOME/.bashrc"
chmod 600 "$OC_HOME/.bashrc"
"#,
    )
}

/// Lightsail prepends a /bin/sh script, so an embedded shebang alone is ignored.
fn bash_user_data(body: &str) -> String {
    format!("#!/bin/sh\nexec /bin/bash <<'CLAWMACDO_HERMES_BOOTSTRAP'\n{body}\nCLAWMACDO_HERMES_BOOTSTRAP\n")
}

fn hermes_service(description: &str, command: &str) -> String {
    format!(
        r#"[Unit]
Description={description}
After=network-online.target
Wants=network-online.target
StartLimitIntervalSec=60
StartLimitBurst=3

[Service]
Type=simple
User=ubuntu
Group=ubuntu
WorkingDirectory=/home/ubuntu
Environment=HOME=/home/ubuntu
Environment=HERMES_HOME=/home/ubuntu/.hermes
Environment=PATH=/home/ubuntu/.local/bin:/home/ubuntu/.opencode/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
ExecStart=/home/ubuntu/.local/bin/hermes {command}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
"#
    )
}

fn generate_user_data(
    env_content: &str,
    config_content: &str,
    aws_cli_config_content: &str,
    opencode_install_block: &str,
    dashboard: bool,
    tunnel: Option<&DashboardTunnel>,
) -> String {
    let sentinel = config::CLOUD_INIT_SENTINEL;
    let env_b64 = base64::engine::general_purpose::STANDARD.encode(env_content.as_bytes());
    let config_b64 = base64::engine::general_purpose::STANDARD.encode(config_content.as_bytes());
    let aws_cli_config_b64 =
        base64::engine::general_purpose::STANDARD.encode(aws_cli_config_content.as_bytes());
    let gateway_unit = hermes_service("Hermes Agent gateway", "gateway run");
    let tunnel_block = tunnel
        .map(|tunnel| {
            let settings = serde_json::json!({
                "password": tunnel.password,
            });
            include_str!("hermes_cloudflare.sh")
                .replace(
                    "__TUNNEL_CONFIG_B64__",
                    &base64::engine::general_purpose::STANDARD.encode(settings.to_string()),
                )
                .replace(
                    "__TUNNEL_READY_B64__",
                    &base64::engine::general_purpose::STANDARD
                        .encode(include_str!("hermes_cloudflare_ready.py")),
                )
        })
        .unwrap_or_default();
    let tunnel_start = if tunnel.is_some() {
        include_str!("hermes_cloudflare_start.sh")
    } else {
        ""
    };
    let dashboard_block = if dashboard {
        let mut unit = hermes_service(
            "Hermes Agent dashboard",
            "dashboard --host 127.0.0.1 --port 9119 --no-open",
        );
        if tunnel.is_some() {
            unit = unit.replace(
                "[Service]\n",
                "[Service]\nEnvironmentFile=/etc/hermes-cloudflared/dashboard.env\n",
            );
        }
        format!("cat > /etc/systemd/system/hermes-dashboard.service <<'UNIT'\n{unit}UNIT\nsystemctl daemon-reload\nsystemctl enable --now hermes-dashboard.service\n")
    } else {
        String::new()
    };
    let body = format!(
        r#"set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
export NEEDRESTART_MODE=a
LOG=/var/log/clawmacdo-hermes-provision.log
exec > >(tee -a "$LOG") 2>&1
trap 'rc=$?; echo "[clawmacdo] Bootstrap failed at line $LINENO (exit $rc)"; exit "$rc"' ERR

ENV_B64='{env_b64}'
CONFIG_B64='{config_b64}'
AWS_CLI_CONFIG_B64='{aws_cli_config_b64}'

echo "[clawmacdo] Installing native Hermes Agent on Lightsail"
apt-get update -y
apt-get upgrade -y
apt-get install -y ca-certificates curl git unzip python3 ufw fail2ban unattended-upgrades

ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
ufw --force enable

echo "[clawmacdo] Installing Hermes for ubuntu"
install -d -m 700 -o ubuntu -g ubuntu /home/ubuntu/.hermes
curl -fsSL https://hermes-agent.nousresearch.com/install.sh -o /tmp/clawmacdo-hermes-install.sh
runuser -u ubuntu -- env HOME=/home/ubuntu HERMES_HOME=/home/ubuntu/.hermes \
  bash /tmp/clawmacdo-hermes-install.sh --non-interactive

echo "[clawmacdo] Configuring Hermes AI provider and Telegram"
printf '%s' "$ENV_B64" | base64 -d > /home/ubuntu/.hermes/.env
printf '%s' "$CONFIG_B64" | base64 -d > /home/ubuntu/.hermes/config.yaml
chown ubuntu:ubuntu /home/ubuntu/.hermes/.env /home/ubuntu/.hermes/config.yaml
chmod 600 /home/ubuntu/.hermes/.env /home/ubuntu/.hermes/config.yaml

if [ -n "$AWS_CLI_CONFIG_B64" ]; then
  install -d -m 700 -o ubuntu -g ubuntu /home/ubuntu/.aws
  printf '%s' "$AWS_CLI_CONFIG_B64" | base64 -d > /home/ubuntu/.aws/config
  chown ubuntu:ubuntu /home/ubuntu/.aws/config
  chmod 600 /home/ubuntu/.aws/config
fi

{opencode_install_block}
{tunnel_block}
echo "[clawmacdo] Starting native Hermes services"
cat > /etc/systemd/system/hermes-gateway.service <<'UNIT'
{gateway_unit}UNIT
systemctl daemon-reload
systemctl enable --now hermes-gateway.service
{dashboard_block}
# Catch immediate crashes instead of declaring a restarting service healthy.
sleep 10
systemctl is-active --quiet hermes-gateway.service
{dashboard_check}
{tunnel_start}
touch {sentinel}
echo "[clawmacdo] Native Hermes Agent bootstrap complete"
"#,
        dashboard_check = if dashboard {
            "systemctl is-active --quiet hermes-dashboard.service"
        } else {
            ""
        },
    );
    bash_user_data(&body)
}

fn print_summary(
    record: &DeployRecord,
    dashboard: bool,
    ai_summary: &str,
    opencode: bool,
    telegram_bot: Option<&str>,
) {
    let divider = "=".repeat(60);
    let ip = &record.ip_address;
    let key = &record.ssh_key_path;

    println!("\n{divider}");
    println!("  Hermes Agent Lightsail Provision Complete");
    println!("{divider}");
    println!("  Hostname:          {}", record.hostname);
    println!("  IP Address:        {ip}");
    println!("  Region:            {}", record.region);
    println!("  Size:              {}", record.size);
    println!();
    println!("  SSH Access:");
    println!("    ssh -i {key} ubuntu@{ip}");
    println!();
    println!("  Hermes data:       /home/ubuntu/.hermes on server");
    println!("  Hermes env:        /home/ubuntu/.hermes/.env on server");
    println!("  AWS CLI config:    /home/ubuntu/.aws/config (Bedrock only)");
    println!("  AI model:          {ai_summary}");
    if let Some(bot) = telegram_bot {
        println!("  Telegram bot:      @{bot}");
        println!(
            "  Telegram pairing:  DM the bot, then run: clawmacdo hermes-telegram-pair --instance {} --code <CODE>",
            record.hostname
        );
    }
    println!(
        "  Gateway logs:      ssh -i {key} ubuntu@{ip} 'sudo journalctl -u hermes-gateway -f'"
    );
    if opencode {
        println!("  OpenCode CLI:      installed for ubuntu (opencode)");
        println!("  OpenCode config:   /home/ubuntu/.config/opencode/opencode.json");
    }
    if dashboard {
        println!("  Dashboard tunnel:  ssh -i {key} -L 9119:127.0.0.1:9119 ubuntu@{ip}");
        println!("                     then open http://127.0.0.1:9119");
    }
    if let Some(url) = &record.dashboard_url {
        println!("  Dashboard HTTPS:   {url}");
        println!("  Quick Tunnel:      URL changes when the tunnel restarts");
        println!("  Current URL:       ssh -i {key} ubuntu@{ip} 'cat /var/lib/hermes-cloudflared/dashboard-url'");
        println!("  Dashboard login:   admin (use the password supplied when provisioning)");
        println!("  Tunnel logs:       ssh -i {key} ubuntu@{ip} 'sudo journalctl -u hermes-cloudflared -f'");
    }
    println!(
        "  Deploy Record:     ~/.clawmacdo/deploys/{}.json",
        record.id
    );
    println!("{divider}\n");
}

fn find_deploy_record(query: &str) -> Result<(String, PathBuf)> {
    let deploys_dir = config::deploys_dir()?;
    if !deploys_dir.exists() {
        bail!("No deploy records found. Provision a Hermes instance first.");
    }
    for entry in std::fs::read_dir(&deploys_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<DeployRecord>(&contents) else {
            continue;
        };
        if record.id == query || record.hostname == query || record.ip_address == query {
            return Ok((record.ip_address, PathBuf::from(record.ssh_key_path)));
        }
    }
    bail!("No deploy record found for '{query}'. Use a deploy ID, hostname, or IP address.");
}

async fn exec_hermes(ip: &str, key: &std::path::Path, args: &str) -> Result<String> {
    let ip_owned = ip.to_string();
    let key_owned = key.to_path_buf();
    let command = format!(
        "env HOME=/home/ubuntu HERMES_HOME=/home/ubuntu/.hermes /home/ubuntu/.local/bin/hermes {args}"
    );
    let output = tokio::task::spawn_blocking(move || {
        ssh::exec_as(&ip_owned, &key_owned, &command, "ubuntu")
    })
    .await??;
    Ok(output)
}

/// Approve a pending Telegram DM pairing code on the native Hermes host.
pub(super) async fn approve_telegram_pairing(
    ip: &str,
    key: &std::path::Path,
    code: &str,
) -> Result<String> {
    validate_telegram_pair_code(code)?;
    let args = format!("pairing approve telegram {}", sh_quote(code.trim()));
    let mut last_error = String::new();
    for attempt in 0..8 {
        match exec_hermes(ip, key, &args).await {
            Ok(output) => return Ok(output),
            Err(err) => {
                last_error = err.to_string();
                if attempt < 7 {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
    bail!(
        "Failed to approve Telegram pairing code '{code}'. \
         DM the bot first so it issues a code, then retry. Last error: {last_error}"
    )
}

/// Approve a Telegram pairing code for an existing Hermes deployment.
pub async fn pair_telegram(instance: &str, code: &str) -> Result<String> {
    validate_telegram_pair_code(code)?;
    let (ip, key) = find_deploy_record(instance)?;
    approve_telegram_pairing(&ip, &key, code).await
}

/// List pending and approved Telegram pairings for an existing Hermes deployment.
pub async fn list_telegram_pairings(instance: &str) -> Result<String> {
    let (ip, key) = find_deploy_record(instance)?;
    exec_hermes(&ip, &key, "pairing list").await
}

pub async fn run(params: HermesLightsailParams) -> Result<DeployRecord> {
    config::ensure_dirs()?;
    let deploy_id = params
        .deploy_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let tx = &params.progress_tx;
    let step_db = &params.db;

    record_step_start(
        step_db,
        &deploy_id,
        1,
        "Resolving Hermes Lightsail parameters",
    );
    progress::emit(
        tx,
        "\n[Step 1/8] Resolving Hermes Agent Lightsail parameters...",
    );
    let tunnel = match dashboard_tunnel(&params) {
        Ok(tunnel) => tunnel,
        Err(err) => {
            record_step_failed(step_db, &deploy_id, 1, &err.to_string());
            return Err(err);
        }
    };
    let dashboard = params.dashboard || tunnel.is_some();

    let region = if params.aws_region.trim().is_empty() {
        "ap-southeast-1".to_string()
    } else {
        params.aws_region.trim().to_string()
    };
    let size = params
        .size
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "s-2vcpu-4gb".to_string());
    let hostname = params
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("hermes-{}", &deploy_id[..8]));
    let telegram_token = params
        .telegram_bot_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let telegram_pair_code = params
        .telegram_pair_code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut telegram_bot_username: Option<String> = None;
    if let Some(token) = telegram_token {
        if let Err(err) = validate_telegram_bot_token(token) {
            record_step_failed(step_db, &deploy_id, 1, &err.to_string());
            return Err(err);
        }
        if params.dry_run {
            progress::emit(
                tx,
                "  Telegram:  bot token format valid (not verified in dry-run)",
            );
        } else {
            match verify_telegram_bot_token(token).await {
                Ok(TelegramVerification::Verified(username)) => {
                    progress::emit(tx, &format!("  Telegram:  verified @{username}"));
                    telegram_bot_username = Some(username);
                }
                Ok(TelegramVerification::Unverified(reason)) => {
                    progress::emit(
                        tx,
                        &format!(
                            "  Telegram:  token accepted locally; getMe unavailable ({reason})"
                        ),
                    );
                    telegram_bot_username = Some("configured".to_string());
                }
                Err(err) => {
                    record_step_failed(step_db, &deploy_id, 1, &err.to_string());
                    return Err(err);
                }
            }
        }
    }
    if let Some(code) = telegram_pair_code {
        if telegram_token.is_none() {
            let err = "A Telegram bot token is required to approve a pairing code.";
            record_step_failed(step_db, &deploy_id, 1, err);
            bail!("{err}");
        }
        if let Err(err) = validate_telegram_pair_code(code) {
            record_step_failed(step_db, &deploy_id, 1, &err.to_string());
            return Err(err);
        }
    }
    let ai_provider = match resolve_ai_provider(&params) {
        Ok(provider) => provider,
        Err(err) => {
            record_step_failed(step_db, &deploy_id, 1, &err.to_string());
            return Err(err);
        }
    };
    // Hermes and the OpenCode CLI both use this single resolved OpenRouter model.
    let openrouter_model = normalized_openrouter_model(&params);
    let bedrock_region = normalized_bedrock_region(&params);
    let bedrock_model = normalized_bedrock_model(&params);
    let (config_content, aws_cli_config_content, ai_summary) = match ai_provider {
        AiProvider::OpenRouter => (
            build_openrouter_config_content(&openrouter_model),
            String::new(),
            format!("OpenRouter {openrouter_model}"),
        ),
        AiProvider::Bedrock => (
            build_bedrock_config_content(&bedrock_region, &bedrock_model),
            build_aws_cli_config_content(&bedrock_region),
            format!("AWS Bedrock {bedrock_model} ({bedrock_region})"),
        ),
    };
    let env_content = build_env_content(&params)?;
    let opencode_install_block = if params.install_opencode {
        build_opencode_install_block(
            params.openrouter_api_key.as_deref(),
            &build_opencode_config_content(&openrouter_model),
        )
    } else {
        "# OpenCode CLI install disabled by --no-opencode\n".to_string()
    };
    // Preview scripts must not reveal the new tunnel credentials, even base64-encoded.
    let preview_tunnel = tunnel.as_ref().map(|_| DashboardTunnel {
        password: "REDACTED".into(),
    });
    let user_data = generate_user_data(
        &env_content,
        &config_content,
        &aws_cli_config_content,
        &opencode_install_block,
        dashboard,
        if params.dry_run {
            preview_tunnel.as_ref()
        } else {
            tunnel.as_ref()
        },
    );
    progress::emit(tx, "  Provider: AWS Lightsail");
    progress::emit(tx, &format!("  Region:   {region}"));
    progress::emit(tx, &format!("  Size:     {size}"));
    progress::emit(tx, &format!("  Hostname: {hostname}"));
    progress::emit(tx, "  Runtime:  native host (terminal backend: local)");
    progress::emit(tx, &format!("  AI Model: {ai_summary}"));
    if tunnel.is_some() {
        progress::emit(
            tx,
            "  Dashboard: Cloudflare Quick Tunnel (URL assigned at startup, login: admin)",
        );
    }
    if params.install_opencode {
        progress::emit(tx, "  OpenCode: CLI v2 will be installed for ubuntu");
    }
    record_step_complete(step_db, &deploy_id, 1);

    if params.dry_run {
        progress::emit(tx, "\n[Dry-run] No AWS resources will be created.");
        if params.json {
            let payload = serde_json::json!({
                "provider": "hermes-lightsail",
                "region": region,
                "size": size,
                "hostname": hostname,
                "runtime": "native",
                "ai_provider": match ai_provider {
                    AiProvider::OpenRouter => "openrouter",
                    AiProvider::Bedrock => "bedrock",
                },
                "ai_model": ai_summary,
                "openrouter_model": openrouter_model,
                "opencode_installed": params.install_opencode,
                "telegram_configured": telegram_token.is_some(),
                "telegram_pair_code_requested": telegram_pair_code.is_some(),
                "dashboard": dashboard,
                "dashboard_url": null,
                "cloudflare_tunnel": tunnel.is_some(),
                "user_data": user_data,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            println!("\n--- Lightsail user-data preview ---\n{user_data}");
        }
        let dry_steps = [
            "Generating SSH key pair",
            "Uploading SSH key to AWS Lightsail",
            "Creating Lightsail instance",
            "Waiting for Lightsail instance",
            "Waiting for SSH",
            "Waiting for Hermes cloud-init",
            "Saving deploy record",
        ];
        for (idx, label) in dry_steps.iter().enumerate() {
            let step = idx as i32 + 2;
            record_step_start(step_db, &deploy_id, step, label);
            record_step_skipped(step_db, &deploy_id, step);
        }
        return Ok(DeployRecord {
            id: deploy_id,
            provider: Some(CloudProviderType::Lightsail),
            droplet_id: 0,
            instance_id: Some("(dry-run)".to_string()),
            hostname,
            ip_address: "0.0.0.0".to_string(),
            region,
            size,
            ssh_key_path: "(dry-run)".to_string(),
            ssh_key_fingerprint: String::new(),
            ssh_key_id: Some("(dry-run)".to_string()),
            resource_group: None,
            backup_restored: None,
            dashboard_url: None,
            created_at: Utc::now(),
        });
    }

    if params.aws_access_key_id.trim().is_empty() || params.aws_secret_access_key.trim().is_empty()
    {
        let err = "AWS credentials required. Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.";
        record_step_failed(step_db, &deploy_id, 1, err);
        bail!("{err}");
    }

    clawmacdo_cloud::lightsail_cli::ensure_aws_cli()?;
    let lightsail = LightsailCliProvider::with_credentials(
        region.clone(),
        params.aws_access_key_id.clone(),
        params.aws_secret_access_key.clone(),
    );

    record_step_start(step_db, &deploy_id, 2, "Generating SSH key pair");
    progress::emit(tx, "\n[Step 2/8] Generating SSH key pair...");
    let keypair = ssh::generate_keypair(&deploy_id)?;
    progress::emit(
        tx,
        &format!("  Key saved: {}", keypair.private_key_path.display()),
    );
    record_step_complete(step_db, &deploy_id, 2);

    record_step_start(step_db, &deploy_id, 3, "Uploading SSH key to AWS Lightsail");
    progress::emit(tx, "\n[Step 3/8] Uploading SSH key to AWS Lightsail...");
    let key_name = format!("clawmacdo-{deploy_id}");
    let key_info = lightsail
        .upload_ssh_key(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key to AWS Lightsail")?;
    progress::emit(tx, &format!("  Key ID: {}", key_info.id));
    record_step_complete(step_db, &deploy_id, 3);

    record_step_start(step_db, &deploy_id, 4, "Creating Lightsail instance");
    progress::emit(
        tx,
        "\n[Step 4/8] Creating Lightsail instance with Hermes Agent cloud-init...",
    );
    let instance_info = lightsail
        .create_instance(CreateInstanceParams {
            name: hostname.clone(),
            region: region.clone(),
            size: size.clone(),
            image: "ubuntu_24_04".to_string(),
            ssh_key_id: key_name.clone(),
            user_data,
            tags: vec!["app=hermes-agent".to_string()],
            customer_email: params.customer_email.clone(),
        })
        .await
        .context("Failed to create Lightsail instance")?;
    progress::emit(tx, &format!("  Instance ID: {}", instance_info.id));
    record_step_complete(step_db, &deploy_id, 4);

    record_step_start(step_db, &deploy_id, 5, "Waiting for Lightsail instance");
    progress::emit(
        tx,
        "\n[Step 5/8] Waiting for Lightsail instance to become active...",
    );
    let instance_info = lightsail
        .wait_for_active(&instance_info.id, 600)
        .await
        .context("Lightsail instance did not become active within 10 minutes")?;
    let ip = instance_info
        .public_ip
        .context("Lightsail instance has no public IP")?;
    progress::emit(tx, &format!("  IP: {ip}"));
    record_step_complete(step_db, &deploy_id, 5);

    record_step_start(step_db, &deploy_id, 6, "Waiting for SSH");
    progress::emit(tx, "\n[Step 6/8] Waiting for SSH...");
    ssh::wait_for_ssh(
        &ip,
        &keypair.private_key_path,
        std::time::Duration::from_secs(300),
        Some("ubuntu"),
    )
    .await
    .context("SSH did not become available within 5 minutes")?;
    progress::emit(tx, "[Step 6/8] SSH ready");
    record_step_complete(step_db, &deploy_id, 6);

    record_step_start(step_db, &deploy_id, 7, "Waiting for Hermes cloud-init");
    progress::emit(
        tx,
        "\n[Step 7/8] Waiting for Hermes Agent cloud-init to finish...",
    );
    if let Err(err) = ssh::wait_for_cloud_init(
        &ip,
        &keypair.private_key_path,
        std::time::Duration::from_secs(1800),
        Some("ubuntu"),
    )
    .await
    {
        record_step_failed(step_db, &deploy_id, 7, &err.to_string());
        return Err(err.into());
    }
    progress::emit(tx, "[Step 7/8] Hermes Agent cloud-init complete");
    let dashboard_url = if tunnel.is_some() {
        match read_dashboard_url(&ip, &keypair.private_key_path).await {
            Ok(url) => {
                progress::emit(tx, &format!("  Dashboard HTTPS: {url}"));
                Some(url)
            }
            Err(err) => {
                record_step_failed(step_db, &deploy_id, 7, &err.to_string());
                return Err(err.context("Failed to read the generated Hermes Quick Tunnel URL"));
            }
        }
    } else {
        None
    };
    record_step_complete(step_db, &deploy_id, 7);

    if let Some(code) = telegram_pair_code {
        progress::emit(tx, "[Step 7/8] Approving Telegram pairing code...");
        match approve_telegram_pairing(&ip, &keypair.private_key_path, code).await {
            Ok(output) => {
                let output = output.trim();
                if !output.is_empty() {
                    progress::emit(tx, &format!("  {output}"));
                }
                progress::emit(tx, "[Step 7/8] Telegram pairing approved");
            }
            Err(err) => {
                progress::emit(
                    tx,
                    &format!(
                        "  Warning: Telegram pairing was not approved: {err}\n  \
                         Retry with: clawmacdo hermes-telegram-pair --instance <deploy-id|hostname|ip> --code {code}"
                    ),
                );
            }
        }
    }

    record_step_start(step_db, &deploy_id, 8, "Saving deploy record");
    progress::emit(tx, "\n[Step 8/8] Saving deploy record...");
    let record = DeployRecord {
        id: deploy_id,
        provider: Some(CloudProviderType::Lightsail),
        droplet_id: 0,
        instance_id: Some(instance_info.id),
        hostname,
        ip_address: ip,
        region,
        size,
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: String::new(),
        ssh_key_id: Some(key_info.id),
        resource_group: None,
        backup_restored: None,
        dashboard_url,
        created_at: Utc::now(),
    };
    let record_path = record.save()?;
    progress::emit(tx, &format!("  Saved: {}", record_path.display()));
    progress::emit(tx, "\n[Step 8/8] Done!");
    record_step_complete(step_db, &record.id, 8);
    progress::emit(tx, "\n[Done] Hermes Agent Lightsail provision complete!");
    if params.json {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        print_summary(
            &record,
            dashboard,
            &ai_summary,
            params.install_opencode && has_value(params.openrouter_api_key.as_deref()),
            telegram_bot_username.as_deref(),
        );
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_repeated_env_assignments() {
        assert!(validate_env_assignment("OPENAI_API_KEY=sk-test").is_ok());
        assert!(validate_env_assignment("_CUSTOM=value").is_ok());
        assert!(validate_env_assignment("1BAD=value").is_err());
        assert!(validate_env_assignment("NO_EQUALS").is_err());
    }

    #[test]
    fn user_data_installs_native_services_without_docker() {
        let user_data = generate_user_data(
            "TELEGRAM_BOT_TOKEN=test\n",
            &build_openrouter_config_content(DEFAULT_OPENROUTER_MODEL),
            "",
            &build_opencode_install_block(
                Some("sk-or-test"),
                &build_opencode_config_content(DEFAULT_OPENROUTER_MODEL),
            ),
            true,
            None,
        );
        assert!(user_data.contains("hermes-agent.nousresearch.com/install.sh"));
        assert!(user_data.contains("--non-interactive"));
        assert!(user_data.contains("User=ubuntu"));
        assert!(user_data.contains("/home/ubuntu/.local/bin/hermes gateway run"));
        assert!(user_data.contains("hermes-dashboard.service"));
        assert!(user_data.contains("https://opencode.ai/v2/install"));
        assert!(!user_data.contains("docker"));
        assert!(user_data.contains(config::CLOUD_INIT_SENTINEL));
        let without_dashboard = generate_user_data("", "", "", "", false, None);
        assert!(!without_dashboard.contains("hermes-dashboard.service"));
        assert!(!without_dashboard.contains("cloudflared"));
    }

    #[test]
    fn quick_tunnel_requires_only_a_dashboard_password() {
        let mut params = openrouter_params("openrouter", Some("sk-or-test"), None);
        assert!(dashboard_tunnel(&params).unwrap().is_none());
        params.cloudflare_tunnel = true;
        assert!(dashboard_tunnel(&params).is_err());
        for password in ["short", "long-but\ninvalid", "                "] {
            params.dashboard_password = Some(password.into());
            assert!(dashboard_tunnel(&params).is_err());
        }
        params.dashboard_password = Some("correct horse battery staple".into());
        assert_eq!(
            dashboard_tunnel(&params).unwrap().unwrap().password,
            "correct horse battery staple"
        );
        params.cloudflare_tunnel = false;
        assert!(dashboard_tunnel(&params).unwrap().is_none());
    }

    #[test]
    fn quick_tunnel_url_rejects_unexpected_or_unsafe_output() {
        assert_eq!(
            parse_quick_tunnel_url("https://test-123.trycloudflare.com\n").unwrap(),
            "https://test-123.trycloudflare.com"
        );
        for url in [
            "",
            "http://test.trycloudflare.com",
            "https://test.trycloudflare.com.evil.com",
            "https://trycloudflare.com",
            "https://-test.trycloudflare.com",
            "https://test.trycloudflare.com/path",
            "https://test.trycloudflare.com:443",
            "https://user@test.trycloudflare.com",
            "https://test.trycloudflare.com\ninjected",
        ] {
            assert!(parse_quick_tunnel_url(url).is_err(), "accepted {url:?}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn tunnel_bootstrap_is_valid_bash_and_checks_auth_before_exposure() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let tunnel = DashboardTunnel {
            password: "quoted ' password $()".into(),
        };
        let script = generate_user_data(
            "",
            "terminal:\n  backend: local\n",
            "",
            "",
            true,
            Some(&tunnel),
        );
        assert!(!script.contains(&tunnel.password));
        assert!(!script.contains("docker"));
        assert!(!script.contains("--host 0.0.0.0"));
        assert!(script.contains("--url http://127.0.0.1:9119"));
        assert!(!script.contains("--token"));
        assert!(!script.contains("LoadCredential="));
        assert!(script.contains("ExecStartPost=+/usr/bin/python3"));
        assert!(script.contains("EnvironmentFile=/etc/hermes-cloudflared/dashboard.env"));
        assert!(
            script
                .find("'public_url': 'https://pending.invalid'")
                .unwrap()
                < script
                    .find("systemctl enable --now hermes-cloudflared.service")
                    .unwrap()
        );
        // Parse the embedded Bash too: the outer sh heredoc is opaque to bash -n.
        let body = script
            .split_once("CLAWMACDO_HERMES_BOOTSTRAP'\n")
            .unwrap()
            .1;
        let body = body.strip_suffix("\nCLAWMACDO_HERMES_BOOTSTRAP\n").unwrap();
        let mut child = Command::new("bash")
            .arg("-n")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    }

    #[test]
    #[cfg(unix)]
    fn lightsail_sh_wrapper_executes_bash_and_preserves_failures() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        for (body, expected) in [
            (
                "set -euo pipefail\nvalues=(native hermes)\nprintf '%s' \"${values[1]}\"",
                0,
            ),
            ("set -euo pipefail\nfalse", 1),
        ] {
            let script = format!(
                "#!/bin/sh\n# Lightsail's prepended initialization\n{}",
                bash_user_data(body)
            );
            let mut child = Command::new("sh")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(script.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(expected));
            if expected == 0 {
                assert_eq!(output.stdout, b"hermes");
            }
        }
    }

    #[test]
    fn build_env_content_adds_telegram_onboarding_fields() {
        let params = HermesLightsailParams {
            deploy_id: None,
            customer_email: "test@example.com".to_string(),
            name: None,
            aws_access_key_id: String::new(),
            aws_secret_access_key: String::new(),
            aws_region: "ap-southeast-1".to_string(),
            size: None,
            env_file: None,
            env_content: Some("NOUS_API_KEY=nous-test".to_string()),
            env_vars: vec!["OPENROUTER_API_KEY=or-test".to_string()],
            ai_provider: String::new(),
            openrouter_api_key: None,
            openrouter_model: DEFAULT_OPENROUTER_MODEL.to_string(),
            bedrock_api_key: Some("bedrock-test".to_string()),
            bedrock_region: DEFAULT_BEDROCK_REGION.to_string(),
            bedrock_model: DEFAULT_BEDROCK_MODEL.to_string(),
            install_opencode: true,
            telegram_bot_token: Some("123456:abc".to_string()),
            telegram_allowed_users: Some("111, 222".to_string()),
            telegram_home_channel: None,
            telegram_pair_code: None,
            dashboard: false,
            cloudflare_tunnel: false,
            dashboard_password: None,
            dry_run: true,
            json: false,
            progress_tx: None,
            db: None,
        };

        let env = build_env_content(&params).unwrap();

        assert!(env.contains("NOUS_API_KEY=nous-test\n"));
        assert!(env.contains("OPENROUTER_API_KEY=or-test\n"));
        assert!(env.contains("AWS_BEARER_TOKEN_BEDROCK=bedrock-test\n"));
        assert!(env.contains("OPENAI_API_KEY=bedrock-test\n"));
        assert!(env.contains("OPENAI_BASE_URL=https://bedrock-mantle.ap-southeast-1.api.aws/v1\n"));
        assert!(env.contains("AWS_REGION=ap-southeast-1\n"));
        assert!(env.contains("TELEGRAM_BOT_TOKEN=123456:abc\n"));
        assert!(env.contains("TELEGRAM_ALLOWED_USERS=111,222\n"));
        assert!(env.contains("TELEGRAM_HOME_CHANNEL=111\n"));
    }

    #[test]
    fn bedrock_config_sets_nova_pro_in_singapore() {
        let config = build_bedrock_config_content(DEFAULT_BEDROCK_REGION, DEFAULT_BEDROCK_MODEL);
        assert!(config.contains("default: 'amazon.nova-pro-v1:0'"));
        assert!(config.contains("provider: custom"));
        assert!(config.contains("base_url: 'https://bedrock-mantle.ap-southeast-1.api.aws/v1'"));
        assert!(config.contains("region: 'ap-southeast-1'"));
    }

    fn openrouter_params(
        ai_provider: &str,
        openrouter_api_key: Option<&str>,
        bedrock_api_key: Option<&str>,
    ) -> HermesLightsailParams {
        HermesLightsailParams {
            deploy_id: None,
            customer_email: "test@example.com".to_string(),
            name: None,
            aws_access_key_id: String::new(),
            aws_secret_access_key: String::new(),
            aws_region: "ap-southeast-1".to_string(),
            size: None,
            env_file: None,
            env_content: None,
            env_vars: Vec::new(),
            ai_provider: ai_provider.to_string(),
            openrouter_api_key: openrouter_api_key.map(str::to_string),
            openrouter_model: DEFAULT_OPENROUTER_MODEL.to_string(),
            bedrock_api_key: bedrock_api_key.map(str::to_string),
            bedrock_region: DEFAULT_BEDROCK_REGION.to_string(),
            bedrock_model: DEFAULT_BEDROCK_MODEL.to_string(),
            install_opencode: true,
            telegram_bot_token: None,
            telegram_allowed_users: None,
            telegram_home_channel: None,
            telegram_pair_code: None,
            dashboard: false,
            cloudflare_tunnel: false,
            dashboard_password: None,
            dry_run: true,
            json: false,
            progress_tx: None,
            db: None,
        }
    }

    #[test]
    fn auto_provider_prefers_openrouter_when_key_present() {
        let params = openrouter_params("", Some("sk-or-test"), Some("bedrock-test"));
        assert_eq!(
            resolve_ai_provider(&params).unwrap(),
            AiProvider::OpenRouter
        );

        let params = openrouter_params("", None, Some("bedrock-test"));
        assert_eq!(resolve_ai_provider(&params).unwrap(), AiProvider::Bedrock);
    }

    #[test]
    fn explicit_provider_requires_its_key() {
        let params = openrouter_params("openrouter", None, Some("bedrock-test"));
        assert!(resolve_ai_provider(&params).is_err());

        let params = openrouter_params("bedrock", Some("sk-or-test"), None);
        assert!(resolve_ai_provider(&params).is_err());
    }

    #[test]
    fn openrouter_config_and_env_use_gpt_6_sol() {
        let params = openrouter_params("", Some("sk-or-test"), None);
        let env = build_env_content(&params).unwrap();
        assert!(env.contains("OPENROUTER_API_KEY=sk-or-test\n"));
        assert!(!env.contains("AWS_BEARER_TOKEN_BEDROCK"));

        let config = build_openrouter_config_content(DEFAULT_OPENROUTER_MODEL);
        assert!(config.contains("default: 'openai/gpt-6-sol'"));
        assert!(config.contains("provider: openrouter"));
        assert!(config.contains("base_url: 'https://openrouter.ai/api/v1'"));
    }

    #[test]
    fn opencode_config_uses_openrouter_model() {
        let config = build_opencode_config_content(DEFAULT_OPENROUTER_MODEL);
        let value: serde_json::Value = serde_json::from_str(&config).unwrap();
        assert_eq!(value["model"], "openrouter/openai/gpt-6-sol");
        assert_eq!(
            value["providers"]["openrouter"]["env"][0],
            "OPENROUTER_API_KEY"
        );
    }

    #[test]
    fn hermes_and_opencode_share_the_same_openrouter_model() {
        let mut params = openrouter_params("", Some("sk-or-test"), None);
        params.openrouter_model = "openai/gpt-6.1-sol".to_string();
        let model = normalized_openrouter_model(&params);

        let hermes = build_openrouter_config_content(&model);
        let opencode: serde_json::Value =
            serde_json::from_str(&build_opencode_config_content(&model)).unwrap();

        assert!(hermes.contains("provider: openrouter"));
        assert!(hermes.contains("default: 'openai/gpt-6.1-sol'"));
        assert_eq!(opencode["model"], "openrouter/openai/gpt-6.1-sol");
    }

    #[test]
    fn validates_telegram_bot_tokens() {
        assert!(validate_telegram_bot_token("123456789:ABCdefGHIjklMNOpqrSTUvwxYZ").is_ok());
        assert!(validate_telegram_bot_token("123456789:abc-def_123").is_ok());
        assert!(validate_telegram_bot_token("").is_err());
        assert!(validate_telegram_bot_token("not-a-token").is_err());
        assert!(validate_telegram_bot_token("abc:secret").is_err());
        assert!(validate_telegram_bot_token("123:bad secret").is_err());
    }

    #[test]
    fn validates_telegram_pair_codes() {
        assert!(validate_telegram_pair_code("XKGH5N7P").is_ok());
        assert!(validate_telegram_pair_code("abcd1234").is_ok());
        assert!(validate_telegram_pair_code("abc").is_err());
        assert!(validate_telegram_pair_code("bad code").is_err());
    }
}
