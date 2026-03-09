use crate::provision::commands::{ssh_as_openclaw_async, ssh_root_async};
use clawmacdo_core::config::{OPENCLAW_HOME, OPENCLAW_USER};
use clawmacdo_core::error::AppError;
use std::path::Path;

/// Step 11a: Configure pnpm directories and settings for openclaw user.
/// Node.js + pnpm already installed globally by cloud-init.
/// Translated from openclaw-ansible/roles/openclaw/tasks/nodejs.yml + openclaw.yml (pnpm config).
/// PProvision.
pub async fn provision(ip: &str, key: &Path) -> Result<(), AppError> {
    let user = OPENCLAW_USER;
    let home = OPENCLAW_HOME;

    // Create pnpm directories for openclaw user
    let mkdirs = format!(
        "mkdir -p {home}/.local/share/pnpm/store {home}/.local/bin && \
         chown -R {user}:{user} {home}/.local",
    );
    ssh_root_async(ip, key, &mkdirs).await?;

    // Configure pnpm for openclaw user
    let pnpm_cfg = format!(
        "pnpm config set global-dir {home}/.local/share/pnpm && \
         pnpm config set global-bin-dir {home}/.local/bin",
    );
    ssh_as_openclaw_async(ip, key, &pnpm_cfg)
        .await
        .map_err(|e| AppError::Provision {
            phase: "pnpm config".into(),
            message: e.to_string(),
        })?;

    // Install global AI CLIs for the openclaw user
    let cli_install = format!(
        "PNPM_HOME={home}/.local/share/pnpm \
         PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         pnpm install -g @anthropic-ai/claude-code @openai/codex @google/gemini-cli",
    );
    ssh_as_openclaw_async(ip, key, &cli_install)
        .await
        .map_err(|e| AppError::Provision {
            phase: "node cli install".into(),
            message: e.to_string(),
        })?;

    // Verify CLI binaries are available to the openclaw user.
    // Use ; instead of && so one missing CLI does not block the whole deploy.
    // claude is required; codex and gemini are optional.
    let cli_verify = format!(
        "PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         claude --version && \
         (codex --version 2>/dev/null || echo 'codex: skipped') && \
         (gemini --version 2>/dev/null || echo 'gemini: skipped')",
    );
    ssh_as_openclaw_async(ip, key, &cli_verify)
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
    ssh_root_async(ip, key, &symlink_cmd)
        .await
        .map_err(|e| AppError::Provision {
            phase: "cli symlinks".into(),
            message: e.to_string(),
        })?;

    Ok(())
}
