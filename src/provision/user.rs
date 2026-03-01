use crate::config::{OPENCLAW_HOME, OPENCLAW_USER};
use crate::error::AppError;
use crate::provision::commands::ssh_root_async;
use std::path::Path;

/// Step 8: Create openclaw system user, configure sudoers, .ssh, and environment.
/// Translated from openclaw-ansible/roles/openclaw/tasks/user.yml.
pub async fn provision(ip: &str, key: &Path, public_key_openssh: &str) -> Result<(), AppError> {
    let user = OPENCLAW_USER;
    let home = OPENCLAW_HOME;

    // Create system user
    ssh_root_async(
        ip,
        key,
        &format!(
            "id -u {user} >/dev/null 2>&1 || \
             useradd --system --create-home --home-dir {home} --shell /bin/bash {user}"
        ),
    )
    .await
    .map_err(|e| AppError::Provision {
        phase: "user creation".into(),
        message: e.to_string(),
    })?;

    // Ensure home directory ownership
    ssh_root_async(
        ip,
        key,
        &format!("chown {user}:{user} {home} && chmod 755 {home}"),
    )
    .await?;

    // Write .bashrc
    let bashrc = format!(
        r#"cat > {home}/.bashrc << 'BASHRCEOF'
# Enable 256 colors
export TERM=xterm-256color
export COLORTERM=truecolor

# pnpm paths
export PNPM_HOME="{home}/.local/share/pnpm"
export PATH="{home}/.local/bin:$PNPM_HOME:$PATH"

# Load OpenClaw environment variables when present
if [ -f "$HOME/.openclaw/.env" ]; then
  set -a
  . "$HOME/.openclaw/.env"
  set +a
fi

# Color support
export CLICOLOR=1
export LS_COLORS='di=34:ln=35:so=32:pi=33:ex=31:bd=34;46:cd=34;43:su=30;41:sg=30;46:tw=30;42:ow=30;43'

# Aliases
alias ls='ls --color=auto'
alias grep='grep --color=auto'
alias ll='ls -lah'

# XDG runtime dir for systemd user services
export XDG_RUNTIME_DIR=/run/user/$(id -u)

# DBus session bus configuration
if [ -z "$DBUS_SESSION_BUS_ADDRESS" ]; then
  export DBUS_SESSION_BUS_ADDRESS="unix:path=${{XDG_RUNTIME_DIR}}/bus"
fi
BASHRCEOF
chown {user}:{user} {home}/.bashrc && chmod 644 {home}/.bashrc"#,
        home = home,
        user = user,
    );
    ssh_root_async(ip, key, &bashrc).await?;

    // Write .bash_profile
    let bash_profile = format!(
        r#"cat > {home}/.bash_profile << 'BPEOF'
# Source .bashrc for login shells
if [ -f ~/.bashrc ]; then
    . ~/.bashrc
fi
BPEOF
chown {user}:{user} {home}/.bash_profile && chmod 644 {home}/.bash_profile"#,
        home = home,
        user = user,
    );
    ssh_root_async(ip, key, &bash_profile).await?;

    // Write sudoers (scoped permissions)
    let sudoers = format!(
        r#"cat > /etc/sudoers.d/{user} << 'SUDOEOF'
# OpenClaw sudo permissions (scoped for security)

# Service control - openclaw service only
{user} ALL=(ALL) NOPASSWD: /usr/bin/systemctl start openclaw
{user} ALL=(ALL) NOPASSWD: /usr/bin/systemctl stop openclaw
{user} ALL=(ALL) NOPASSWD: /usr/bin/systemctl restart openclaw
{user} ALL=(ALL) NOPASSWD: /usr/bin/systemctl status openclaw
{user} ALL=(ALL) NOPASSWD: /usr/bin/systemctl enable openclaw
{user} ALL=(ALL) NOPASSWD: /usr/bin/systemctl disable openclaw
{user} ALL=(ALL) NOPASSWD: /usr/bin/systemctl daemon-reload

# Tailscale diagnostics + connect/disconnect
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale status
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale up *
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale down
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale ip *
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale version
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale ping *
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale whois *

# Journal access - openclaw logs only
{user} ALL=(ALL) NOPASSWD: /usr/bin/journalctl -u openclaw *
SUDOEOF
chmod 440 /etc/sudoers.d/{user} && chown root:root /etc/sudoers.d/{user}
visudo -cf /etc/sudoers.d/{user}"#,
        user = user,
    );
    ssh_root_async(ip, key, &sudoers).await.map_err(|e| AppError::Provision {
        phase: "sudoers".into(),
        message: e.to_string(),
    })?;

    // Setup .ssh/authorized_keys with deploy key
    let ssh_setup = format!(
        r#"mkdir -p {home}/.ssh && \
chmod 700 {home}/.ssh && \
echo '{pubkey}' > {home}/.ssh/authorized_keys && \
chmod 600 {home}/.ssh/authorized_keys && \
chown -R {user}:{user} {home}/.ssh"#,
        home = home,
        user = user,
        pubkey = public_key_openssh,
    );
    ssh_root_async(ip, key, &ssh_setup).await?;

    // Enable lingering for systemd user services
    ssh_root_async(
        ip,
        key,
        &format!("loginctl enable-linger {user}"),
    )
    .await?;

    // Create runtime directory
    let runtime_dir = format!(
        r#"OPENCLAW_UID=$(id -u {user}) && \
mkdir -p /run/user/$OPENCLAW_UID && \
chown {user}:{user} /run/user/$OPENCLAW_UID && \
chmod 700 /run/user/$OPENCLAW_UID"#,
        user = user,
    );
    ssh_root_async(ip, key, &runtime_dir).await?;

    // If a backup was restored to /root/.openclaw, move it into the openclaw home.
    let restore_backup = format!(
        r#"if [ -d /root/.openclaw ]; then \
mkdir -p {home}/.openclaw && \
cp -a /root/.openclaw/. {home}/.openclaw/ && \
chown -R {user}:{user} {home}/.openclaw && \
chmod 700 {home}/.openclaw && \
rm -rf /root/.openclaw; \
fi"#,
        home = home,
        user = user,
    );
    ssh_root_async(ip, key, &restore_backup).await?;

    Ok(())
}
