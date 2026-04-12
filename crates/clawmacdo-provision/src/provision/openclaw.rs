use crate::provision::commands::{ssh_as_openclaw_with_user_async, ssh_root_as_async};
use clawmacdo_core::config::{OPENCLAW_HOME, OPENCLAW_USER};
use clawmacdo_core::error::AppError;
use clawmacdo_ssh as ssh;
use std::path::Path;

/// Create directory structure, write .env, install OpenClaw via pnpm.
/// Translated from openclaw-ansible/roles/openclaw/tasks/openclaw.yml + openclaw-release.yml.
/// PProvision.
#[allow(clippy::too_many_arguments)]
pub async fn provision(
    ip: &str,
    key: &Path,
    anthropic_api_key: &str,
    anthropic_setup_token: &str,
    openai_key: &str,
    gemini_key: &str,
    byteplus_ark_api_key: &str,
    opencode_api_key: &str,
    whatsapp_phone_number: &str,
    telegram_bot_token: &str,
    ssh_user: &str,
    openclaw_version: &str,
) -> Result<(), AppError> {
    let user = OPENCLAW_USER;
    let home = OPENCLAW_HOME;
    let config_dir = format!("{home}/.openclaw");

    // Create directory structure with proper permissions
    let cd = &config_dir;
    let mkdirs = format!(
        r#"mkdir -p {cd}/sessions {cd}/credentials {cd}/data {cd}/logs {cd}/agents/main/agent {cd}/workspace && \
chmod 700 {cd} {cd}/credentials {cd}/agents/main/agent && \
chown -R {user}:{user} {cd}"#,
    );
    ssh_root_as_async(ip, key, &mkdirs, ssh_user).await?;

    // Write .env with provider credentials via SCP (avoids shell heredoc injection)
    let env_content = format!(
        "ANTHROPIC_API_KEY={anthropic_api_key}\n\
         ANTHROPIC_SETUP_TOKEN={anthropic_setup_token}\n\
         OPENAI_API_KEY={openai_key}\n\
         GEMINI_API_KEY={gemini_key}\n\
         BYTEPLUS_API_KEY={byteplus_ark_api_key}\n\
         OPENCODE_API_KEY={opencode_api_key}\n\
         WHATSAPP_PHONE_NUMBER={whatsapp_phone_number}\n\
         TELEGRAM_BOT_TOKEN={telegram_bot_token}\n",
    );
    let gateway_env_content = format!(
        "ANTHROPIC_API_KEY={anthropic_api_key}\n\
            OPENAI_API_KEY={openai_key}\n\
            GEMINI_API_KEY={gemini_key}\n\
            BYTEPLUS_API_KEY={byteplus_ark_api_key}\n\
            OPENCODE_API_KEY={opencode_api_key}\n\
            WHATSAPP_PHONE_NUMBER={whatsapp_phone_number}\n\
            TELEGRAM_BOT_TOKEN={telegram_bot_token}\n",
    );
    let scp_user = if ssh_user == "root" { "root" } else { ssh_user };
    let key_owned = key.to_path_buf();
    let ip_owned = ip.to_string();
    let scp_user_owned = scp_user.to_string();
    let env_bytes = env_content.into_bytes();
    let gateway_env_bytes = gateway_env_content.into_bytes();
    tokio::task::spawn_blocking(move || {
        ssh::scp_upload_bytes(
            &ip_owned,
            &key_owned,
            &env_bytes,
            "/tmp/.env_upload",
            0o600,
            &scp_user_owned,
        )
    })
    .await
    .map_err(|e| AppError::Ssh(format!("spawn_blocking join: {e}")))??;

    ssh_root_as_async(
        ip,
        key,
        &format!(
            "mv /tmp/.env_upload {cd}/.env && chmod 600 {cd}/.env && chown {user}:{user} {cd}/.env"
        ),
        ssh_user,
    )
    .await?;

    let key_owned = key.to_path_buf();
    let ip_owned = ip.to_string();
    let scp_user_owned = scp_user.to_string();
    tokio::task::spawn_blocking(move || {
        ssh::scp_upload_bytes(
            &ip_owned,
            &key_owned,
            &gateway_env_bytes,
            "/tmp/.gateway_env_upload",
            0o600,
            &scp_user_owned,
        )
    })
    .await
    .map_err(|e| AppError::Ssh(format!("spawn_blocking join: {e}")))??;

    ssh_root_as_async(
        ip,
        key,
        &format!(
            "mv /tmp/.gateway_env_upload {cd}/gateway.env && chmod 600 {cd}/gateway.env && chown {user}:{user} {cd}/gateway.env"
        ),
        ssh_user,
    )
    .await?;

    // Configure Claude Code with enhanced settings and better error handling
    let claude_cfg = format!(
        r#"mkdir -p {home}/.claude {home}/.config/claude && \
cat > {home}/.claude/api-key-helper.sh << 'CCHELPEREOF'
#!/usr/bin/env bash
# Enhanced API key helper with error handling and fallbacks
set -euo pipefail

# Try to source from .openclaw/.env first
if [ -f "$HOME/.openclaw/.env" ]; then
  set -a
  . "$HOME/.openclaw/.env" 2>/dev/null || true
  set +a
fi

# Fallback to environment variable
if [ -n "${{ANTHROPIC_API_KEY:-}}" ]; then
  printf '%s' "$ANTHROPIC_API_KEY"
  exit 0
fi

# If no key found, provide helpful error
echo "Error: No Anthropic API key found. Please set ANTHROPIC_API_KEY in ~/.openclaw/.env" >&2
exit 1
CCHELPEREOF
chmod 700 {home}/.claude && chmod 700 {home}/.claude/api-key-helper.sh && \
cat > {home}/.claude/settings.json << 'CCSETTINGSEOF'
{{
  "apiKeyHelper": "{home}/.claude/api-key-helper.sh",
  "forceLoginMethod": "console",
  "defaultModel": "claude-3-5-sonnet-20241022",
  "maxTokens": 8192,
  "temperature": 0.7,
  "autoSave": true,
  "theme": "dark",
  "editor": {{
    "fontSize": 14,
    "wordWrap": "on",
    "tabSize": 2,
    "minimap": {{
      "enabled": false
    }}
  }},
  "ai": {{
    "codeCompletion": true,
    "suggestions": true,
    "contextAware": true
  }},
  "telemetry": {{
    "enabled": false
  }}
}}
CCSETTINGSEOF
chmod 600 {home}/.claude/settings.json && \
chown -R {user}:{user} {home}/.claude {home}/.config/claude && \
echo 'Claude Code configuration complete!' && \
echo 'Configuration file: {home}/.claude/settings.json' && \
echo 'API key helper: {home}/.claude/api-key-helper.sh'"#,
    );
    ssh_root_as_async(ip, key, &claude_cfg, ssh_user).await?;

    // Break hardlinked files under extensions (OpenClaw security rejects hardlinks).
    let normalize_extensions = format!(
        r#"if [ -d {cd}/extensions ]; then \
find {cd}/extensions -type f -links +1 -exec sh -c 'for f do cp -p "$f" "$f.__clawmacdo_tmp" && mv -f "$f.__clawmacdo_tmp" "$f"; done' sh {{}} +; \
fi && \
chown -R {user}:{user} {cd} && \
chmod 700 {cd}"#,
    );
    ssh_root_as_async(ip, key, &normalize_extensions, ssh_user).await?;

    // Install openclaw globally. Try pnpm first (user-scoped), fall back to npm (system-wide as root).
    // On some Tencent Ubuntu images, npm global install fails with ENOENT due to /bin/sh quirks,
    // so pnpm is preferred. If pnpm also fails to make it accessible, install via npm as root.
    let version_spec = openclaw_version.to_string();
    let install_cmd = format!(
        "PNPM_HOME={home}/.local/share/pnpm \
         PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         pnpm install -g openclaw@{version_spec} 2>&1 || true",
    );
    ssh_as_openclaw_with_user_async(ip, key, &install_cmd, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "openclaw install (pnpm)".into(),
            message: e.to_string(),
        })?;

    // Verify pnpm install succeeded; if not, fall back to npm global install as root.
    let verify_pnpm = format!(
        "PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         openclaw --version 2>/dev/null || echo OPENCLAW_NOT_FOUND",
    );
    let pnpm_result = ssh_as_openclaw_with_user_async(ip, key, &verify_pnpm, ssh_user)
        .await
        .unwrap_or_default();
    if pnpm_result.contains("OPENCLAW_NOT_FOUND") {
        // Fallback: install as root via npm (installs to /usr/lib/node_modules, binary at /usr/bin/openclaw)
        ssh_root_as_async(
            ip,
            key,
            &format!("npm install -g openclaw@{version_spec} 2>&1 || pnpm install -g openclaw@{version_spec} 2>&1"),
            ssh_user,
        )
        .await
        .map_err(|e| AppError::Provision {
            phase: "openclaw install (npm fallback)".into(),
            message: e.to_string(),
        })?;
    }

    // Verify installation (check both user and system paths)
    let verify_cmd = format!(
        "PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         openclaw --version",
    );
    let version = ssh_as_openclaw_with_user_async(ip, key, &verify_cmd, ssh_user).await?;
    println!("  OpenClaw version: {}", version.trim());

    if !anthropic_api_key.trim().is_empty() {
        // Warm up Claude in headless mode so first manual run is not blocked by onboarding prompts.
        // This is best-effort and should never fail the deploy.
        let claude_bootstrap = format!(
            "PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
             HOME={home} \
             timeout 240s claude -p \"health check\" --output-format text --max-turns 1 >/dev/null 2>&1 || true",
        );
        if let Err(e) = ssh_as_openclaw_with_user_async(ip, key, &claude_bootstrap, ssh_user).await
        {
            eprintln!("  Warning: Claude bootstrap failed; continuing: {e}");
        }
    }

    Ok(())
}
