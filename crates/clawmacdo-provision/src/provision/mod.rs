pub mod commands;
pub mod docker;
pub mod firewall;
pub mod nodejs;
pub mod openclaw;
pub mod system_tools;
pub mod tailscale;
pub mod user;

use clawmacdo_core::error::AppError;
use clawmacdo_ui::progress;
use std::path::Path;
use tokio::sync::mpsc;

/// Callback type for step start notifications (step_number, label).
pub type StepStartFn = Box<dyn Fn(i32, &str) + Send + Sync>;
/// Callback type for step completion notifications (step_number).
pub type StepDoneFn = Box<dyn Fn(i32) + Send + Sync>;

/// Options for the SSH-based provisioning steps (steps 9–14).
pub struct ProvisionOpts<'a> {
    pub anthropic_api_key: &'a str,
    pub anthropic_setup_token: &'a str,
    pub openai_key: &'a str,
    pub gemini_key: &'a str,
    pub byteplus_ark_api_key: &'a str,
    pub whatsapp_phone_number: &'a str,
    pub telegram_bot_token: &'a str,
    pub public_key_openssh: &'a str,
    pub hostname: &'a str,
    pub openclaw_version: &'a str,
    pub tailscale: bool,
    pub tailscale_auth_key: Option<&'a str>,
    /// SSH username to connect as (e.g. "root" for DigitalOcean, "ubuntu" for AWS Lightsail).
    /// Defaults to "root" if None.
    pub ssh_user: Option<&'a str>,
    /// Optional channel for streaming progress to the web UI (SSE).
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
    /// Callback invoked when a provision step starts.
    pub on_step: Option<StepStartFn>,
    /// Callback invoked when a provision step completes.
    pub on_step_done: Option<StepDoneFn>,
}

fn notify_step(opts: &ProvisionOpts<'_>, step: i32, label: &str) {
    if let Some(cb) = &opts.on_step {
        cb(step, label);
    }
}

fn notify_step_done(opts: &ProvisionOpts<'_>, step: i32) {
    if let Some(cb) = &opts.on_step_done {
        cb(step);
    }
}

/// Run all SSH-based provisioning steps in order.
///
/// Called after cloud-init has finished installing base packages.
/// Expects SSH access as root to `ip` using `key`.
///
/// Steps 9–14 of the 16-step deploy flow.
pub async fn run(ip: &str, key: &Path, opts: &ProvisionOpts<'_>) -> Result<(), AppError> {
    let tx = &opts.progress_tx;
    let ssh_user = opts.ssh_user.unwrap_or("root");

    // Step 9: Create openclaw user + sudoers + .ssh
    notify_step(opts, 9, "Creating openclaw user and configuring access");
    progress::emit(
        tx,
        "\n[Step 9/16] Creating openclaw user and configuring access...",
    );
    user::provision(ip, key, opts.public_key_openssh, ssh_user).await?;
    progress::emit(tx, "  User 'openclaw' created with SSH access");
    notify_step_done(opts, 9);

    // Step 10: Harden firewall (fail2ban, UFW, DOCKER-USER)
    notify_step(opts, 10, "Hardening firewall");
    progress::emit(
        tx,
        "\n[Step 10/16] Hardening firewall (fail2ban, UFW, Docker isolation)...",
    );
    firewall::provision(ip, key, opts.tailscale, ssh_user).await?;
    progress::emit(tx, "  Firewall hardened");
    notify_step_done(opts, 10);

    // Step 11: Configure Docker daemon
    notify_step(opts, 11, "Configuring Docker daemon");
    progress::emit(tx, "\n[Step 11/16] Configuring Docker daemon...");
    docker::provision(ip, key, ssh_user).await?;
    progress::emit(tx, "  Docker daemon configured");
    notify_step_done(opts, 11);

    // Step 12: Install pnpm + configure for openclaw user
    notify_step(opts, 12, "Setting up Node.js/pnpm");
    progress::emit(tx, "\n[Step 12/16] Setting up Node.js/pnpm...");
    nodejs::provision(ip, key, ssh_user).await?;
    progress::emit(tx, "  pnpm configured");
    notify_step_done(opts, 12);

    // Step 13: Install OpenClaw as openclaw user
    notify_step(opts, 13, "Installing OpenClaw");
    progress::emit(tx, "\n[Step 13/16] Installing OpenClaw...");
    openclaw::provision(
        ip,
        key,
        opts.anthropic_api_key,
        opts.anthropic_setup_token,
        opts.openai_key,
        opts.gemini_key,
        opts.byteplus_ark_api_key,
        opts.whatsapp_phone_number,
        opts.telegram_bot_token,
        ssh_user,
        opts.openclaw_version,
    )
    .await?;
    progress::emit(tx, "  OpenClaw installed");
    notify_step_done(opts, 13);

    // System tools (vim config, git config) — not a numbered step, runs as part of setup
    system_tools::provision(ip, key, ssh_user).await?;

    // Step 14: Optional Tailscale
    notify_step(opts, 14, "Tailscale VPN");
    if opts.tailscale {
        progress::emit(tx, "\n[Step 14/16] Installing Tailscale VPN...");
        match tailscale::provision(ip, key, opts.hostname, opts.tailscale_auth_key, ssh_user)
            .await?
        {
            tailscale::TailscaleProvisionStatus::Connected => {
                progress::emit(tx, "  Tailscale installed and connected");
            }
            tailscale::TailscaleProvisionStatus::InstalledOnly => {
                progress::emit(
                    tx,
                    "  Tailscale installed (complete `tailscale up` from a privileged shell to connect)",
                );
            }
            tailscale::TailscaleProvisionStatus::ConnectFailed(err) => {
                progress::emit(
                    tx,
                    "  Tailscale installed, but auto-connect failed; complete `tailscale up` from a privileged shell",
                );
                progress::emit(tx, &format!("  Tailscale auto-connect error: {err}"));
            }
        }
    } else {
        progress::emit(tx, "\n[Step 14/16] Tailscale skipped (not enabled)");
    }
    notify_step_done(opts, 14);

    // Post-provisioning: ensure root login is restricted to pubkey-only.
    // Cloud-init sets PermitRootLogin to prohibit-password, but enforce it
    // here as a safety net in case the config was modified during provisioning.
    commands::ssh_root_as_async(
        ip,
        key,
        "sed -i 's/^PermitRootLogin yes/PermitRootLogin prohibit-password/' /etc/ssh/sshd_config && \
         sed -i 's/^PermitRootLogin yes/PermitRootLogin prohibit-password/' /etc/ssh/sshd_config.d/*.conf 2>/dev/null || true && \
         systemctl restart sshd 2>/dev/null || systemctl restart ssh",
        ssh_user,
    )
    .await?;

    Ok(())
}
