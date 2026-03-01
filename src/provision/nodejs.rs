use crate::config::{OPENCLAW_HOME, OPENCLAW_USER};
use crate::error::AppError;
use crate::provision::commands::{ssh_as_openclaw_async, ssh_root_async};
use std::path::Path;

/// Step 11a: Configure pnpm directories and settings for openclaw user.
/// Node.js + pnpm already installed globally by cloud-init.
/// Translated from openclaw-ansible/roles/openclaw/tasks/nodejs.yml + openclaw.yml (pnpm config).
pub async fn provision(ip: &str, key: &Path) -> Result<(), AppError> {
    let user = OPENCLAW_USER;
    let home = OPENCLAW_HOME;

    // Create pnpm directories for openclaw user
    let mkdirs = format!(
        "mkdir -p {home}/.local/share/pnpm/store {home}/.local/bin && \
         chown -R {user}:{user} {home}/.local",
        home = home,
        user = user,
    );
    ssh_root_async(ip, key, &mkdirs).await?;

    // Configure pnpm for openclaw user
    let pnpm_cfg = format!(
        "pnpm config set global-dir {home}/.local/share/pnpm && \
         pnpm config set global-bin-dir {home}/.local/bin",
        home = home,
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
        home = home,
    );
    ssh_as_openclaw_async(ip, key, &cli_install)
        .await
        .map_err(|e| AppError::Provision {
            phase: "node cli install".into(),
            message: e.to_string(),
        })?;

    // Verify CLI binaries are available to the openclaw user
    let cli_verify = format!(
        "PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin \
         HOME={home} \
         claude --version && codex --version && gemini --version",
        home = home,
    );
    ssh_as_openclaw_async(ip, key, &cli_verify)
        .await
        .map_err(|e| AppError::Provision {
            phase: "node cli verify".into(),
            message: e.to_string(),
        })?;

    Ok(())
}
