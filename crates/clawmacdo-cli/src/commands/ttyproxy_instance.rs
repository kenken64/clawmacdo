use anyhow::{bail, Result};
use base64::Engine;
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use std::path::PathBuf;

const OPENCLAW_INSTANCE_KEY: &str = "OPENCLAW_INSTANCE";
const OPENCLAW_WORKSPACE: &str = "/home/openclaw/.openclaw/workspace";
const DEFAULT_ENV_FILE: &str = "claw-ttyproxy/.env";

pub struct TtyproxyInstanceSetParams {
    pub instance: String,
    pub openclaw_instance: String,
    pub env_file: Option<String>,
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn validate_remote_path_arg(label: &str, value: &str) -> Result<()> {
    if value.contains('\0') || value.contains('\n') || value.contains('\r') {
        bail!("{label} cannot contain NUL or newline characters");
    }
    Ok(())
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

fn build_set_instance_script(workspace: &str, env_file: Option<&str>, value: &str) -> String {
    let value_b64 = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());
    let env_file = env_file.unwrap_or(DEFAULT_ENV_FILE);
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

WORKSPACE={workspace}
REQUESTED_ENV_FILE={env_file}
ENV_KEY={env_key}
VALUE_B64={value_b64}

fail() {{
  echo "ttyproxy-instance: ERROR: $*" >&2
  exit 1
}}

[ -d "$WORKSPACE" ] || fail "OpenClaw workspace not found: $WORKSPACE"

case "$REQUESTED_ENV_FILE" in
  /*) ENV_FILE="$REQUESTED_ENV_FILE" ;;
  *) ENV_FILE="$WORKSPACE/$REQUESTED_ENV_FILE" ;;
esac

if [ ! -f "$ENV_FILE" ]; then
  echo "ttyproxy-instance: .env files found under $WORKSPACE:" >&2
  find "$WORKSPACE" -maxdepth 4 -type f -name .env -print 2>/dev/null >&2 || true
  fail ".env not found: $ENV_FILE. Pass --env-file <workspace-relative-or-absolute-path>."
fi
[ ! -L "$ENV_FILE" ] || fail "Refusing to update symlinked .env: $ENV_FILE"

WORKSPACE_REAL="$(readlink -f "$WORKSPACE")"
ENV_REAL="$(readlink -f "$ENV_FILE")"
case "$ENV_REAL" in
  "$WORKSPACE_REAL"/*) ;;
  *) fail "Refusing to update .env outside workspace: $ENV_REAL" ;;
esac

VALUE="$(printf '%s' "$VALUE_B64" | base64 -d)"
[ -n "$VALUE" ] || fail "decoded $ENV_KEY value is empty"

BACKUP="$ENV_REAL.clawmacdo.$(date -u +%Y%m%d%H%M%S).bak"
cp -p "$ENV_REAL" "$BACKUP"
chmod 600 "$BACKUP" 2>/dev/null || true

ESCAPED="$(printf '%s' "$VALUE" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
NEW_LINE="$ENV_KEY=\"$ESCAPED\""
export NEW_LINE ENV_KEY

TMP="$ENV_REAL.clawmacdo.tmp"
awk 'BEGIN {{ done = 0 }}
  index($0, ENVIRON["ENV_KEY"] "=") == 1 {{ print ENVIRON["NEW_LINE"]; done = 1; next }}
  {{ print }}
  END {{ if (!done) print ENVIRON["NEW_LINE"] }}' "$ENV_REAL" > "$TMP"
chmod 600 "$TMP"
mv "$TMP" "$ENV_REAL"

echo "ttyproxy-instance: updated"
echo "env_file=$ENV_REAL"
echo "key=$ENV_KEY"
echo "backup=$BACKUP"
"#,
        workspace = sh_quote(workspace),
        env_file = sh_quote(env_file),
        env_key = sh_quote(OPENCLAW_INSTANCE_KEY),
        value_b64 = sh_quote(&value_b64),
    )
}

pub async fn run(params: TtyproxyInstanceSetParams) -> Result<()> {
    let value = params.openclaw_instance.trim();
    if value.is_empty() {
        bail!("--openclaw-instance cannot be empty");
    }
    if value.contains('\0') || value.contains('\n') || value.contains('\r') {
        bail!("--openclaw-instance cannot contain NUL or newline characters");
    }
    if let Some(env_file) = params.env_file.as_deref() {
        validate_remote_path_arg("--env-file", env_file)?;
    }

    let (ip, key, provider) = find_deploy_record(&params.instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let script = build_set_instance_script(OPENCLAW_WORKSPACE, params.env_file.as_deref(), value);

    println!("Updating {OPENCLAW_INSTANCE_KEY} on {ip}...");
    println!(
        "  Target: {}/{}",
        OPENCLAW_WORKSPACE,
        params.env_file.as_deref().unwrap_or(DEFAULT_ENV_FILE)
    );
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &script, ssh_user).await?;

    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        println!("  {line}");
    }

    println!("{OPENCLAW_INSTANCE_KEY} updated. Restart the tty proxy to pick up the change.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn set_instance_script_defaults_to_claw_ttyproxy_env() {
        let script =
            build_set_instance_script("/home/openclaw/.openclaw/workspace", None, "my-instance");
        assert!(script.contains("REQUESTED_ENV_FILE='claw-ttyproxy/.env'"));
        assert!(script.contains("ENV_KEY='OPENCLAW_INSTANCE'"));
        assert!(script.contains("Refusing to update .env outside workspace"));
        assert!(script.contains("Refusing to update symlinked .env"));
    }

    #[test]
    fn set_instance_script_replaces_or_appends_key() {
        let script =
            build_set_instance_script("/home/openclaw/.openclaw/workspace", None, "my-instance");
        assert!(script.contains(r#"index($0, ENVIRON["ENV_KEY"] "=") == 1"#));
        assert!(script.contains(r#"END { if (!done) print ENVIRON["NEW_LINE"] }"#));
        assert!(script.contains("BACKUP=\"$ENV_REAL.clawmacdo."));
    }

    #[test]
    fn set_instance_script_does_not_embed_raw_value() {
        let script = build_set_instance_script(
            "/home/openclaw/.openclaw/workspace",
            None,
            "raw-instance-value",
        );
        assert!(!script.contains("raw-instance-value"));
        assert!(script.contains("VALUE_B64="));
    }

    #[test]
    fn set_instance_script_honors_env_file_override() {
        let script = build_set_instance_script(
            "/home/openclaw/.openclaw/workspace",
            Some("other-proxy/.env"),
            "my-instance",
        );
        assert!(script.contains("REQUESTED_ENV_FILE='other-proxy/.env'"));
    }

    #[test]
    fn remote_path_arg_rejects_newlines() {
        assert!(validate_remote_path_arg("--env-file", "claw-ttyproxy/.env").is_ok());
        assert!(validate_remote_path_arg("--env-file", "bad\n.env").is_err());
    }
}
