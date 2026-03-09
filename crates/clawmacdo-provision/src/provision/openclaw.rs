use crate::provision::commands::{ssh_as_openclaw_async, ssh_root_async};
use clawmacdo_core::config::{OPENCLAW_HOME, OPENCLAW_USER};
use clawmacdo_core::error::AppError;
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
    whatsapp_phone_number: &str,
    telegram_bot_token: &str,
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
    ssh_root_async(ip, key, &mkdirs).await?;

    // Write .env with provider credentials (chmod 600 for security)
    let write_env = format!(
        r#"cat > {cd}/.env << 'ENVEOF'
ANTHROPIC_API_KEY={anthropic_api_key}
ANTHROPIC_SETUP_TOKEN={anthropic_setup_token}
OPENAI_API_KEY={openai_key}
GEMINI_API_KEY={gemini_key}
WHATSAPP_PHONE_NUMBER={whatsapp_phone_number}
TELEGRAM_BOT_TOKEN={telegram_bot_token}
ENVEOF
chmod 600 {cd}/.env && chown {user}:{user} {cd}/.env"#,
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
    );
    ssh_root_async(ip, key, &claude_cfg).await?;

    // Break hardlinked files under extensions (OpenClaw security rejects hardlinks).
    let normalize_extensions = format!(
        r#"if [ -d {cd}/extensions ]; then \
find {cd}/extensions -type f -links +1 -exec sh -c 'for f do cp -p "$f" "$f.__clawmacdo_tmp" && mv -f "$f.__clawmacdo_tmp" "$f"; done' sh {{}} +; \
fi && \
chown -R {user}:{user} {cd} && \
chmod 700 {cd}"#,
    );
    ssh_root_async(ip, key, &normalize_extensions).await?;

    // Install openclaw globally. Try pnpm first (user-scoped), fall back to npm (system-wide as root).
    // On some Tencent Ubuntu images, npm global install fails with ENOENT due to /bin/sh quirks,
    // so pnpm is preferred. If pnpm also fails to make it accessible, install via npm as root.
    let install_cmd = format!(
        "PNPM_HOME={home}/.local/share/pnpm \
         PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         pnpm install -g openclaw@latest 2>&1 || true",
    );
    ssh_as_openclaw_async(ip, key, &install_cmd)
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
    let pnpm_result = ssh_as_openclaw_async(ip, key, &verify_pnpm)
        .await
        .unwrap_or_default();
    if pnpm_result.contains("OPENCLAW_NOT_FOUND") {
        // Fallback: install as root via npm (installs to /usr/lib/node_modules, binary at /usr/bin/openclaw)
        ssh_root_async(
            ip,
            key,
            "npm install -g openclaw@latest 2>&1 || pnpm install -g openclaw@latest 2>&1",
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
    let version = ssh_as_openclaw_async(ip, key, &verify_cmd).await?;
    println!("  OpenClaw version: {}", version.trim());

    if !anthropic_api_key.trim().is_empty() {
        // Warm up Claude in headless mode so first manual run is not blocked by onboarding prompts.
        // This is best-effort and should never fail the deploy.
        let claude_bootstrap = format!(
            "PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
             HOME={home} \
             timeout 240s claude -p \"health check\" --output-format text --max-turns 1 >/dev/null 2>&1 || true",
        );
        if let Err(e) = ssh_as_openclaw_async(ip, key, &claude_bootstrap).await {
            eprintln!("  Warning: Claude bootstrap failed; continuing: {e}");
        }
    }

    Ok(())
}
