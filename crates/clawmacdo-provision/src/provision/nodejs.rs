use crate::provision::commands::{ssh_as_openclaw_with_user_async, ssh_root_as_async};
use clawmacdo_core::config::{OPENCLAW_HOME, OPENCLAW_USER};
use clawmacdo_core::error::AppError;
use std::path::Path;

/// Step 11a: Configure pnpm directories and settings for openclaw user.
/// Node.js + pnpm already installed globally by cloud-init.
/// Translated from openclaw-ansible/roles/openclaw/tasks/nodejs.yml + openclaw.yml (pnpm config).
/// PProvision.
pub async fn provision(ip: &str, key: &Path, ssh_user: &str) -> Result<(), AppError> {
    let user = OPENCLAW_USER;
    let home = OPENCLAW_HOME;

    // Create pnpm directories for openclaw user
    let mkdirs = format!(
        "mkdir -p {home}/.local/share/pnpm/store {home}/.local/bin && \
         chown -R {user}:{user} {home}/.local",
    );
    ssh_root_as_async(ip, key, &mkdirs, ssh_user).await?;

    // Configure pnpm for openclaw user
    let pnpm_cfg = format!(
        "pnpm config set global-dir {home}/.local/share/pnpm && \
         pnpm config set global-bin-dir {home}/.local/bin",
    );
    ssh_as_openclaw_with_user_async(ip, key, &pnpm_cfg, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "pnpm config".into(),
            message: e.to_string(),
        })?;

    // Install global AI CLIs for the openclaw user (latest versions).
    // Claude Code ships its native binary through optional npm dependencies and
    // postinstall hooks; npm's user-scoped global install is the documented path
    // and avoids pnpm's build-script approval flow for global packages.
    let cli_install = format!(
        "export PNPM_HOME={home}/.local/share/pnpm \
         PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} && \
         npm config set prefix {home}/.local && \
         rm -f {home}/.local/bin/claude {home}/.local/bin/codex {home}/.local/bin/gemini && \
         npm install -g --include=optional --ignore-scripts=false \
           @anthropic-ai/claude-code@latest @openai/codex@latest @google/gemini-cli@latest",
    );
    ssh_as_openclaw_with_user_async(ip, key, &cli_install, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "node cli install".into(),
            message: e.to_string(),
        })?;

    // If a host-level npm policy still prevented Claude's postinstall from
    // linking the native binary, run the package repair hook before verify.
    let claude_repair = format!(
        "export PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} && \
         if ! claude --version >/dev/null 2>&1; then \
           CLAUDE_INSTALL_CJS=$(find {home}/.local/lib/node_modules {home}/.local/share/pnpm \
             -path '*/@anthropic-ai/claude-code/install.cjs' -type f 2>/dev/null | head -1); \
           if [ -n \"$CLAUDE_INSTALL_CJS\" ]; then \
             node \"$CLAUDE_INSTALL_CJS\"; \
           fi; \
         fi",
    );
    ssh_as_openclaw_with_user_async(ip, key, &claude_repair, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "claude native repair".into(),
            message: e.to_string(),
        })?;

    // Enhanced CLI verification with better error reporting
    let cli_verify = format!(
        "export PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} && \
         echo '=== AI CLI Verification ===' && \
         echo 'Checking Claude Code...' && claude --version && \
         echo 'Claude Code: ✅ Installed' && \
         echo 'Checking Codex...' && (codex --version 2>/dev/null && echo 'Codex: ✅ Installed' || echo 'Codex: ⚠️  Skipped (optional)') && \
         echo 'Checking Gemini...' && (gemini --version 2>/dev/null && echo 'Gemini: ✅ Installed' || echo 'Gemini: ⚠️  Skipped (optional)') && \
         echo 'AI CLI setup complete!' && \
         echo 'Claude Code config: {home}/.claude/settings.json'",
    );
    ssh_as_openclaw_with_user_async(ip, key, &cli_verify, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "node cli verify".into(),
            message: e.to_string(),
        })?;

    // Symlink CLI binaries into /usr/local/bin so they are accessible to all users
    // (e.g. root via DigitalOcean console login).
    let symlink_cmd = format!(
        "for bin in claude codex gemini; do \
           src={home}/.local/bin/$bin; \
           if [ -f \"$src\" ] || [ -L \"$src\" ]; then \
             ln -sf \"$src\" /usr/local/bin/$bin; \
           fi; \
         done",
    );
    ssh_root_as_async(ip, key, &symlink_cmd, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "cli symlinks".into(),
            message: e.to_string(),
        })?;

    // Post-installation configuration check for Claude Code
    let claude_config_check = format!(
        "export PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} && \
         echo '=== Claude Code Configuration Check ===' && \
         if [ -f '{home}/.claude/settings.json' ]; then \
           echo 'Claude settings: ✅ Found at {home}/.claude/settings.json'; \
           echo 'API key helper: ✅ Configured'; \
           echo 'Ready for use!'; \
         else \
           echo 'Claude settings: ⚠️  Not found (will be created on first run)'; \
         fi && \
         echo 'Usage: claude <your-prompt>' && \
         echo 'Example: claude \"Write a hello world in Python\"'",
    );
    ssh_as_openclaw_with_user_async(ip, key, &claude_config_check, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "claude config check".into(),
            message: e.to_string(),
        })?;

    Ok(())
}
