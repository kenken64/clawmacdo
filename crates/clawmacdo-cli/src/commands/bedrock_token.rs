use anyhow::{bail, Result};
use base64::Engine;
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use std::path::PathBuf;

const BEDROCK_TOKEN_KEY: &str = "AWS_BEARER_TOKEN_BEDROCK";
const OPENCLAW_WORKSPACE: &str = "/home/openclaw/.openclaw/workspace";

pub struct BedrockTokenSetParams {
    pub instance: String,
    pub bearer_token: String,
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

fn build_set_token_script(workspace: &str, env_file: Option<&str>, bearer_token: &str) -> String {
    let token_b64 = base64::engine::general_purpose::STANDARD.encode(bearer_token.as_bytes());
    let env_file = env_file.unwrap_or_default();
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH"

WORKSPACE={workspace}
REQUESTED_ENV_FILE={env_file}
ENV_KEY={env_key}
TOKEN_B64={token_b64}

fail() {{
  echo "bedrock-token: ERROR: $*" >&2
  exit 1
}}

[ -d "$WORKSPACE" ] || fail "OpenClaw workspace not found: $WORKSPACE"

resolve_env_file() {{
  if [ -n "$REQUESTED_ENV_FILE" ]; then
    case "$REQUESTED_ENV_FILE" in
      /*) printf '%s\n' "$REQUESTED_ENV_FILE" ;;
      *) printf '%s\n' "$WORKSPACE/$REQUESTED_ENV_FILE" ;;
    esac
    return 0
  fi

  for candidate in \
    "$WORKSPACE/claw-tty-proxy/.env" \
    "$WORKSPACE/tty-proxy-claw/.env" \
    "$WORKSPACE/claw-tty/.env" \
    "$WORKSPACE/tty-proxy/.env" \
    "$WORKSPACE/ollama-tty-proxy/.env" \
    "$WORKSPACE/claw/.env"; do
    if [ -f "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  find "$WORKSPACE" -maxdepth 4 -type f -name .env 2>/dev/null | while IFS= read -r file; do
    rel_file="${{file#"$WORKSPACE"/}}"
    lower_file="$(printf '%s' "$rel_file" | tr '[:upper:]' '[:lower:]')"
    case "$lower_file" in
      *claw*|*tty*|*proxy*|*ollama*)
        printf '%s\n' "$file"
        exit 0
        ;;
    esac
  done
}}

ENV_FILE="$(resolve_env_file || true)"
if [ -z "$ENV_FILE" ]; then
  echo "bedrock-token: searched under $WORKSPACE" >&2
  find "$WORKSPACE" -maxdepth 4 -type f -name .env -print 2>/dev/null >&2 || true
  fail "tty proxy .env not found. Pass --env-file <workspace-relative-or-absolute-path>."
fi

[ -f "$ENV_FILE" ] || fail ".env file not found: $ENV_FILE"
[ ! -L "$ENV_FILE" ] || fail "Refusing to update symlinked .env: $ENV_FILE"

WORKSPACE_REAL="$(readlink -f "$WORKSPACE")"
ENV_REAL="$(readlink -f "$ENV_FILE")"
case "$ENV_REAL" in
  "$WORKSPACE_REAL"/*) ;;
  *) fail "Refusing to update .env outside workspace: $ENV_REAL" ;;
esac

if ! command -v dotenvx >/dev/null 2>&1; then
  command -v curl >/dev/null 2>&1 || fail "curl is required to install dotenvx"
  if ! curl -sfS https://dotenvx.sh/install.sh | sh >/tmp/clawmacdo-dotenvx-install.log 2>&1; then
    if ! curl -sfS https://dotenvx.sh | sh >/tmp/clawmacdo-dotenvx-install.log 2>&1; then
      cat /tmp/clawmacdo-dotenvx-install.log >&2 || true
      fail "dotenvx install failed"
    fi
  fi
  export PATH="$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH"
fi
command -v dotenvx >/dev/null 2>&1 || fail "dotenvx installed but was not found in PATH"

TOKEN="$(printf '%s' "$TOKEN_B64" | base64 -d)"
[ -n "$TOKEN" ] || fail "decoded Bedrock bearer token is empty"

if grep -Eq 'encrypted:' "$ENV_REAL" && ! grep -Eq '^DOTENV_PUBLIC_KEY=' "$ENV_REAL"; then
  fail "encrypted .env is missing DOTENV_PUBLIC_KEY; refusing to mix a new dotenvx keypair with existing ciphertext"
fi

BACKUP="$ENV_REAL.clawmacdo.$(date -u +%Y%m%d%H%M%S).bak"
cp -p "$ENV_REAL" "$BACKUP"
chmod 600 "$BACKUP" 2>/dev/null || true

ENV_DIR="$(dirname "$ENV_REAL")"
ENV_NAME="$(basename "$ENV_REAL")"
cd "$ENV_DIR"

set +e
dotenvx set "$ENV_KEY" --encrypt -f "$ENV_NAME" -- "$TOKEN" >/tmp/clawmacdo-dotenvx-set.log 2>&1
STATUS=$?
if [ "$STATUS" -ne 0 ]; then
  dotenvx set "$ENV_KEY" -f "$ENV_NAME" -- "$TOKEN" >/tmp/clawmacdo-dotenvx-set.log 2>&1
  STATUS=$?
fi
set -e
unset TOKEN TOKEN_B64

if [ "$STATUS" -ne 0 ]; then
  cat /tmp/clawmacdo-dotenvx-set.log >&2 || true
  cp -p "$BACKUP" "$ENV_REAL" || true
  fail "dotenvx failed to set encrypted Bedrock token; restored backup"
fi

if ! grep -Eq "^${{ENV_KEY}}=([\"']?)encrypted:" "$ENV_REAL"; then
  cp -p "$BACKUP" "$ENV_REAL" || true
  fail "dotenvx did not write an encrypted $ENV_KEY value; restored backup"
fi

chmod 600 "$ENV_REAL"
echo "bedrock-token: updated"
echo "env_file=$ENV_REAL"
echo "backup=$BACKUP"
"#,
        workspace = sh_quote(workspace),
        env_file = sh_quote(env_file),
        env_key = sh_quote(BEDROCK_TOKEN_KEY),
        token_b64 = sh_quote(&token_b64),
    )
}

pub async fn run(params: BedrockTokenSetParams) -> Result<()> {
    let token = params.bearer_token.trim();
    if token.is_empty() {
        bail!("--bearer-token cannot be empty");
    }
    if let Some(env_file) = params.env_file.as_deref() {
        validate_remote_path_arg("--env-file", env_file)?;
    }

    let (ip, key, provider) = find_deploy_record(&params.instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let script = build_set_token_script(OPENCLAW_WORKSPACE, params.env_file.as_deref(), token);

    println!("Updating Bedrock bearer token on {ip}...");
    println!("  Target: OpenClaw workspace tty proxy .env");
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &script, ssh_user).await?;

    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        println!("  {line}");
    }

    println!("Bedrock bearer token updated with dotenvx encryption.");
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
    fn set_token_script_uses_dotenvx_encryption() {
        let script = build_set_token_script(
            "/home/openclaw/.openclaw/workspace",
            Some("claw-tty-proxy/.env"),
            "secret-token",
        );
        assert!(
            script.contains("dotenvx set \"$ENV_KEY\" --encrypt -f \"$ENV_NAME\" -- \"$TOKEN\"")
        );
        assert!(script.contains("rel_file=\"${file#\"$WORKSPACE\"/}\""));
        assert!(script.contains("AWS_BEARER_TOKEN_BEDROCK"));
        assert!(script.contains("encrypted .env is missing DOTENV_PUBLIC_KEY"));
        assert!(script.contains("Refusing to update .env outside workspace"));
    }

    #[test]
    fn set_token_script_does_not_embed_raw_token() {
        let script = build_set_token_script(
            "/home/openclaw/.openclaw/workspace",
            None,
            "raw-secret-token",
        );
        assert!(!script.contains("raw-secret-token"));
        assert!(script.contains("TOKEN_B64="));
    }

    #[test]
    fn remote_path_arg_rejects_newlines() {
        assert!(validate_remote_path_arg("--env-file", "claw-tty-proxy/.env").is_ok());
        assert!(validate_remote_path_arg("--env-file", "bad\n.env").is_err());
    }
}
