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
pub const DEFAULT_HERMES_IMAGE: &str = "nousresearch/hermes-agent:latest";
pub const DEFAULT_BEDROCK_REGION: &str = "ap-southeast-1";
pub const DEFAULT_BEDROCK_MODEL: &str = "amazon.nova-pro-v1:0";

pub struct HermesLightsailParams {
    pub deploy_id: Option<String>,
    pub customer_email: String,
    pub name: Option<String>,
    pub aws_access_key_id: String,
    pub aws_secret_access_key: String,
    pub aws_region: String,
    pub size: Option<String>,
    pub image: String,
    pub env_file: Option<PathBuf>,
    pub env_content: Option<String>,
    pub env_vars: Vec<String>,
    pub bedrock_api_key: Option<String>,
    pub bedrock_region: String,
    pub bedrock_model: String,
    pub telegram_bot_token: Option<String>,
    pub telegram_allowed_users: Option<String>,
    pub telegram_home_channel: Option<String>,
    pub dashboard: bool,
    pub mount_docker_socket: bool,
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

fn bedrock_mantle_base_url(region: &str) -> String {
    format!("https://bedrock-mantle.{region}.api.aws/v1")
}

fn yaml_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn build_bedrock_config_content(region: &str, model: &str) -> String {
    let base_url = bedrock_mantle_base_url(region);
    format!(
        "model:\n  default: {}\n  provider: custom\n  base_url: {}\nbedrock:\n  region: {}\n",
        yaml_single_quote(model),
        yaml_single_quote(&base_url),
        yaml_single_quote(region),
    )
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
    if let Some(api_key) = params.bedrock_api_key.as_deref() {
        let region = normalized_bedrock_region(params);
        let base_url = bedrock_mantle_base_url(&region);
        append_env_assignment(&mut env, "AWS_BEARER_TOKEN_BEDROCK", api_key);
        append_env_assignment(&mut env, "OPENAI_API_KEY", api_key);
        append_env_assignment(&mut env, "OPENAI_BASE_URL", &base_url);
        append_env_assignment(&mut env, "AWS_REGION", &region);
        append_env_assignment(&mut env, "AWS_DEFAULT_REGION", &region);
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
    Ok(env)
}

fn generate_user_data(
    image: &str,
    env_content: &str,
    config_content: &str,
    aws_cli_config_content: &str,
    dashboard: bool,
    mount_docker_socket: bool,
) -> String {
    let sentinel = config::CLOUD_INIT_SENTINEL;
    let image_q = sh_quote(image);
    let env_b64 = base64::engine::general_purpose::STANDARD.encode(env_content.as_bytes());
    let config_b64 = base64::engine::general_purpose::STANDARD.encode(config_content.as_bytes());
    let aws_cli_config_b64 =
        base64::engine::general_purpose::STANDARD.encode(aws_cli_config_content.as_bytes());
    let docker_socket_mount = if mount_docker_socket {
        "  -v /var/run/docker.sock:/var/run/docker.sock \\\n"
    } else {
        ""
    };
    let dashboard_block = if dashboard {
        r#"docker rm -f hermes-dashboard >/dev/null 2>&1 || true
docker run -d \
  --name hermes-dashboard \
  --restart unless-stopped \
  --network host \
  -v /opt/hermes-data:/opt/data \
  --env-file "$ENV_FILE" \
  "$HERMES_IMAGE" dashboard --host 127.0.0.1 --no-open
"#
        .to_string()
    } else {
        "docker rm -f hermes-dashboard >/dev/null 2>&1 || true\n".to_string()
    };

    format!(
        r#"#!/bin/bash
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
LOG=/var/log/clawmacdo-hermes-provision.log
exec > >(tee -a "$LOG") 2>&1

HERMES_IMAGE={image_q}
ENV_FILE=/opt/hermes-provision/hermes.env
ENV_B64='{env_b64}'
CONFIG_FILE=/opt/hermes-data/config.yaml
CONFIG_B64='{config_b64}'
AWS_CLI_CONFIG_B64='{aws_cli_config_b64}'

echo "[clawmacdo] Starting Hermes Agent Lightsail bootstrap"

apt-get update -y
apt-get upgrade -y
apt-get install -y ca-certificates curl gnupg unzip ufw docker.io fail2ban unattended-upgrades

command -v docker >/dev/null 2>&1 || curl -fsSL https://get.docker.com | sh
systemctl enable --now docker

if ! command -v aws >/dev/null 2>&1; then
  case "$(uname -m)" in
    aarch64|arm64) AWSCLI_ARCH=aarch64 ;;
    *) AWSCLI_ARCH=x86_64 ;;
  esac
  if curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-${{AWSCLI_ARCH}}.zip" -o /tmp/awscliv2.zip \
      && rm -rf /tmp/aws \
      && unzip -q /tmp/awscliv2.zip -d /tmp \
      && /tmp/aws/install --update >/dev/null 2>&1; then
    echo "[clawmacdo] AWS CLI installed"
  else
    echo "[clawmacdo] Warning: AWS CLI install failed; Hermes Bedrock API-key mode can still run"
  fi
fi

ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
ufw --force enable

mkdir -p /opt/hermes-data /opt/hermes-provision
cat > "$ENV_FILE" <<'ENVEOF'
# Managed by clawmacdo hermes-provision.
ENVEOF
if [ -n "$ENV_B64" ]; then
  printf '%s' "$ENV_B64" | base64 -d >> "$ENV_FILE"
fi
grep -q '^HERMES_UID=' "$ENV_FILE" || printf 'HERMES_UID=10000\n' >> "$ENV_FILE"
grep -q '^HERMES_GID=' "$ENV_FILE" || printf 'HERMES_GID=10000\n' >> "$ENV_FILE"
chmod 600 "$ENV_FILE"

if [ -n "$CONFIG_B64" ]; then
  printf '%s' "$CONFIG_B64" | base64 -d > "$CONFIG_FILE"
  chmod 640 "$CONFIG_FILE"
fi

if [ -n "$AWS_CLI_CONFIG_B64" ]; then
  mkdir -p /root/.aws /opt/hermes-data/.aws
  printf '%s' "$AWS_CLI_CONFIG_B64" | base64 -d > /root/.aws/config
  printf '%s' "$AWS_CLI_CONFIG_B64" | base64 -d > /opt/hermes-data/.aws/config
  chmod 700 /root/.aws /opt/hermes-data/.aws
  chmod 600 /root/.aws/config /opt/hermes-data/.aws/config
fi

docker pull "$HERMES_IMAGE"

docker rm -f hermes >/dev/null 2>&1 || true
docker run -d \
  --name hermes \
  --restart unless-stopped \
  --network host \
  -v /opt/hermes-data:/opt/data \
{docker_socket_mount}  --env-file "$ENV_FILE" \
  "$HERMES_IMAGE" gateway run

{dashboard_block}
docker ps --filter name=hermes

touch {sentinel}
echo "[clawmacdo] Hermes Agent bootstrap complete"
"#
    )
}

fn print_summary(
    record: &DeployRecord,
    dashboard: bool,
    bedrock_region: &str,
    bedrock_model: &str,
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
    println!("  Hermes data:       /opt/hermes-data on server");
    println!("  Hermes env:        /opt/hermes-provision/hermes.env on server");
    println!("  AWS CLI config:    /root/.aws/config and /opt/hermes-data/.aws/config");
    println!("  AI model:          AWS Bedrock {bedrock_model} ({bedrock_region})");
    println!("  Gateway logs:      ssh -i {key} ubuntu@{ip} 'sudo docker logs -f hermes'");
    if dashboard {
        println!("  Dashboard tunnel:  ssh -i {key} -L 9119:127.0.0.1:9119 ubuntu@{ip}");
        println!("                     then open http://127.0.0.1:9119");
    }
    println!(
        "  Deploy Record:     ~/.clawmacdo/deploys/{}.json",
        record.id
    );
    println!("{divider}\n");
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
    let image = if params.image.trim().is_empty() {
        DEFAULT_HERMES_IMAGE.to_string()
    } else {
        params.image.trim().to_string()
    };
    if params
        .bedrock_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        let err =
            "AWS Bedrock API key required. Set AWS_BEARER_TOKEN_BEDROCK or pass --bedrock-api-key.";
        record_step_failed(step_db, &deploy_id, 1, err);
        bail!("{err}");
    }
    let bedrock_region = normalized_bedrock_region(&params);
    let bedrock_model = normalized_bedrock_model(&params);
    let config_content = build_bedrock_config_content(&bedrock_region, &bedrock_model);
    let aws_cli_config_content = build_aws_cli_config_content(&bedrock_region);
    let env_content = build_env_content(&params)?;
    let user_data = generate_user_data(
        &image,
        &env_content,
        &config_content,
        &aws_cli_config_content,
        params.dashboard,
        params.mount_docker_socket,
    );
    progress::emit(tx, "  Provider: AWS Lightsail");
    progress::emit(tx, &format!("  Region:   {region}"));
    progress::emit(tx, &format!("  Size:     {size}"));
    progress::emit(tx, &format!("  Hostname: {hostname}"));
    progress::emit(tx, &format!("  Image:    {image}"));
    progress::emit(tx, &format!("  AI Model: AWS Bedrock {bedrock_model}"));
    progress::emit(tx, &format!("  AI Region: {bedrock_region}"));
    record_step_complete(step_db, &deploy_id, 1);

    if params.dry_run {
        progress::emit(tx, "\n[Dry-run] No AWS resources will be created.");
        if params.json {
            let payload = serde_json::json!({
                "provider": "hermes-lightsail",
                "region": region,
                "size": size,
                "hostname": hostname,
                "image": image,
                "bedrock_region": bedrock_region,
                "bedrock_model": bedrock_model,
                "dashboard": params.dashboard,
                "mount_docker_socket": params.mount_docker_socket,
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
    ssh::wait_for_cloud_init(
        &ip,
        &keypair.private_key_path,
        std::time::Duration::from_secs(1800),
        Some("ubuntu"),
    )
    .await
    .context("Hermes Agent cloud-init did not complete within 30 minutes")?;
    progress::emit(tx, "[Step 7/8] Hermes Agent cloud-init complete");
    record_step_complete(step_db, &deploy_id, 7);

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
        print_summary(&record, params.dashboard, &bedrock_region, &bedrock_model);
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
    fn user_data_runs_gateway_and_optional_dashboard() {
        let user_data = generate_user_data(
            DEFAULT_HERMES_IMAGE,
            "TELEGRAM_BOT_TOKEN=test\n",
            &build_bedrock_config_content(DEFAULT_BEDROCK_REGION, DEFAULT_BEDROCK_MODEL),
            &build_aws_cli_config_content(DEFAULT_BEDROCK_REGION),
            true,
            true,
        );

        assert!(user_data.contains("docker run -d"));
        assert!(user_data.contains("AWS_CLI_CONFIG_B64="));
        assert!(user_data.contains("awscli-exe-linux-${AWSCLI_ARCH}.zip"));
        assert!(user_data.contains("/opt/hermes-data/.aws/config"));
        assert!(user_data.contains("\"$HERMES_IMAGE\" gateway run"));
        assert!(user_data.contains("hermes-dashboard"));
        assert!(user_data.contains("/var/run/docker.sock:/var/run/docker.sock"));
        assert!(user_data.contains(config::CLOUD_INIT_SENTINEL));
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
            image: DEFAULT_HERMES_IMAGE.to_string(),
            env_file: None,
            env_content: Some("NOUS_API_KEY=nous-test".to_string()),
            env_vars: vec!["OPENROUTER_API_KEY=or-test".to_string()],
            bedrock_api_key: Some("bedrock-test".to_string()),
            bedrock_region: DEFAULT_BEDROCK_REGION.to_string(),
            bedrock_model: DEFAULT_BEDROCK_MODEL.to_string(),
            telegram_bot_token: Some("123456:abc".to_string()),
            telegram_allowed_users: Some("111, 222".to_string()),
            telegram_home_channel: None,
            dashboard: false,
            mount_docker_socket: false,
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
}
