use crate::config::{OPENCLAW_HOME, OPENCLAW_USER};
use crate::error::AppError;
use crate::provision::commands::{ssh_as_openclaw_async, ssh_root_async};
use std::path::Path;

/// Create directory structure, write .env, install OpenClaw via pnpm.
/// Translated from openclaw-ansible/roles/openclaw/tasks/openclaw.yml + openclaw-release.yml.
pub async fn provision(
    ip: &str,
    key: &Path,
    anthropic_key: &str,
    openai_key: &str,
    gemini_key: &str,
    whatsapp_phone_number: &str,
    telegram_bot_token: &str,
) -> Result<(), AppError> {
    let user = OPENCLAW_USER;
    let home = OPENCLAW_HOME;
    let config_dir = format!("{home}/.openclaw");

    // Create directory structure with proper permissions
    let mkdirs = format!(
        r#"mkdir -p {cd}/sessions {cd}/credentials {cd}/data {cd}/logs {cd}/agents/main/agent {cd}/workspace && \
chmod 700 {cd} {cd}/credentials {cd}/agents/main/agent && \
chown -R {user}:{user} {cd}"#,
        cd = config_dir,
        user = user,
    );
    ssh_root_async(ip, key, &mkdirs).await?;

    // Write .env with API keys (chmod 600 for security)
    let write_env = format!(
        r#"cat > {cd}/.env << 'ENVEOF'
ANTHROPIC_API_KEY={anthropic_key}
OPENAI_API_KEY={openai_key}
GEMINI_API_KEY={gemini_key}
WHATSAPP_PHONE_NUMBER={whatsapp_phone_number}
TELEGRAM_BOT_TOKEN={telegram_bot_token}
ENVEOF
chmod 600 {cd}/.env && chown {user}:{user} {cd}/.env"#,
        cd = config_dir,
        user = user,
        anthropic_key = anthropic_key,
        openai_key = openai_key,
        gemini_key = gemini_key,
        whatsapp_phone_number = whatsapp_phone_number,
        telegram_bot_token = telegram_bot_token,
    );
    ssh_root_async(ip, key, &write_env).await?;

    // Configure Claude Code to use Anthropic API key from ~/.openclaw/.env (no interactive login required).
    let claude_cfg = format!(
        r#"mkdir -p {home}/.claude && \
cat > {home}/.claude/api-key-helper.sh << 'CCHELPEREOF'
#!/usr/bin/env bash
if [ -f "$HOME/.openclaw/.env" ]; then
  set -a
  . "$HOME/.openclaw/.env"
  set +a
fi
printf '%s' "${{ANTHROPIC_API_KEY:-}}"
CCHELPEREOF
chmod 700 {home}/.claude && chmod 700 {home}/.claude/api-key-helper.sh && \
cat > {home}/.claude/settings.json << 'CCSETTINGSEOF'
{{
  "apiKeyHelper": "{home}/.claude/api-key-helper.sh",
  "forceLoginMethod": "console"
}}
CCSETTINGSEOF
chmod 600 {home}/.claude/settings.json && \
chown -R {user}:{user} {home}/.claude"#,
        home = home,
        user = user,
    );
    ssh_root_async(ip, key, &claude_cfg).await?;

    // Break hardlinked files under extensions (OpenClaw security rejects hardlinks).
    let normalize_extensions = format!(
        r#"if [ -d {cd}/extensions ]; then \
find {cd}/extensions -type f -links +1 -exec sh -c 'for f do cp -p "$f" "$f.__clawmacdo_tmp" && mv -f "$f.__clawmacdo_tmp" "$f"; done' sh {{}} +; \
fi && \
chown -R {user}:{user} {cd} && \
chmod 700 {cd}"#,
        cd = config_dir,
        user = user,
    );
    ssh_root_async(ip, key, &normalize_extensions).await?;

    // Install openclaw globally as openclaw user via pnpm
    let install_cmd = format!(
        "PNPM_HOME={home}/.local/share/pnpm \
         PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         pnpm install -g openclaw@latest",
        home = home,
    );
    ssh_as_openclaw_async(ip, key, &install_cmd)
        .await
        .map_err(|e| AppError::Provision {
            phase: "openclaw install".into(),
            message: e.to_string(),
        })?;

    // Verify installation
    let verify_cmd = format!(
        "PNPM_HOME={home}/.local/share/pnpm \
         PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         openclaw --version",
        home = home,
    );
    let version = ssh_as_openclaw_async(ip, key, &verify_cmd).await?;
    println!("  OpenClaw version: {}", version.trim());

    if !anthropic_key.trim().is_empty() {
        // Warm up Claude in headless mode so first manual run is not blocked by onboarding prompts.
        let claude_bootstrap = format!(
            "PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
             HOME={home} \
             claude -p \"health check\" --output-format text --max-turns 1 >/dev/null",
            home = home,
        );
        ssh_as_openclaw_async(ip, key, &claude_bootstrap)
            .await
            .map_err(|e| AppError::Provision {
                phase: "claude bootstrap".into(),
                message: e.to_string(),
            })?;
    }

    Ok(())
}
