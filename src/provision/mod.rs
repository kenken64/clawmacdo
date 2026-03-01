pub mod commands;
mod docker;
mod firewall;
mod nodejs;
mod openclaw;
mod system_tools;
mod tailscale;
mod user;

use crate::error::AppError;
use crate::progress;
use std::path::Path;
use tokio::sync::mpsc;

/// Options for the SSH-based provisioning steps (steps 9–14).
pub struct ProvisionOpts<'a> {
    pub anthropic_key: &'a str,
    pub openai_key: &'a str,
    pub gemini_key: &'a str,
    pub whatsapp_phone_number: &'a str,
    pub telegram_bot_token: &'a str,
    pub public_key_openssh: &'a str,
    pub tailscale: bool,
    /// Optional channel for streaming progress to the web UI (SSE).
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
}

/// Run all SSH-based provisioning steps in order.
///
/// Called after cloud-init has finished installing base packages.
/// Expects SSH access as root to `ip` using `key`.
///
/// Steps 9–14 of the 16-step deploy flow.
pub async fn run(
    ip: &str,
    key: &Path,
    opts: &ProvisionOpts<'_>,
) -> Result<(), AppError> {
    let tx = &opts.progress_tx;

    // Step 9: Create openclaw user + sudoers + .ssh
    progress::emit(tx, "\n[Step 9/16] Creating openclaw user and configuring access...");
    user::provision(ip, key, opts.public_key_openssh).await?;
    progress::emit(tx, "  User 'openclaw' created with SSH access");

    // Step 10: Harden firewall (fail2ban, UFW, DOCKER-USER)
    progress::emit(tx, "\n[Step 10/16] Hardening firewall (fail2ban, UFW, Docker isolation)...");
    firewall::provision(ip, key, opts.tailscale).await?;
    progress::emit(tx, "  Firewall hardened");

    // Step 11: Configure Docker daemon
    progress::emit(tx, "\n[Step 11/16] Configuring Docker daemon...");
    docker::provision(ip, key).await?;
    progress::emit(tx, "  Docker daemon configured");

    // Step 12: Install pnpm + configure for openclaw user
    progress::emit(tx, "\n[Step 12/16] Setting up Node.js/pnpm...");
    nodejs::provision(ip, key).await?;
    progress::emit(tx, "  pnpm configured");

    // Step 13: Install OpenClaw as openclaw user
    progress::emit(tx, "\n[Step 13/16] Installing OpenClaw...");
    openclaw::provision(
        ip,
        key,
        opts.anthropic_key,
        opts.openai_key,
        opts.gemini_key,
        opts.whatsapp_phone_number,
        opts.telegram_bot_token,
    )
    .await?;
    progress::emit(tx, "  OpenClaw installed");

    // System tools (vim config, git config) — not a numbered step, runs as part of setup
    system_tools::provision(ip, key).await?;

    // Step 14: Optional Tailscale
    if opts.tailscale {
        progress::emit(tx, "\n[Step 14/16] Installing Tailscale VPN...");
        tailscale::provision(ip, key).await?;
        progress::emit(tx, "  Tailscale installed (run `sudo tailscale up` on server to connect)");
    } else {
        progress::emit(tx, "\n[Step 14/16] Tailscale skipped (not enabled)");
    }

    Ok(())
}
