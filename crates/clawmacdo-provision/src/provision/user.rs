use crate::provision::commands::ssh_root_as_async;
use clawmacdo_core::config::{OPENCLAW_HOME, OPENCLAW_USER};
use clawmacdo_core::error::AppError;
use clawmacdo_ssh as ssh;
use std::path::Path;

/// Step 8: Create openclaw system user, configure sudoers, .ssh, and environment.
/// Translated from openclaw-ansible/roles/openclaw/tasks/user.yml.
/// PProvision.
pub async fn provision(
    ip: &str,
    key: &Path,
    public_key_openssh: &str,
    ssh_user: &str,
) -> Result<(), AppError> {
    let user = OPENCLAW_USER;
    let home = OPENCLAW_HOME;

    // Create system user
    ssh_root_as_async(
        ip,
        key,
        &format!(
            "id -u {user} >/dev/null 2>&1 || \
             useradd --system --create-home --home-dir {home} --shell /bin/bash {user}"
        ),
        ssh_user,
    )
    .await
    .map_err(|e| AppError::Provision {
        phase: "user creation".into(),
        message: e.to_string(),
    })?;

    // Ensure home directory ownership
    ssh_root_as_async(
        ip,
        key,
        &format!("chown {user}:{user} {home} && chmod 755 {home}"),
        ssh_user,
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
    );
    ssh_root_as_async(ip, key, &bashrc, ssh_user).await?;

    // Write .bash_profile
    let bash_profile = format!(
        r#"cat > {home}/.bash_profile << 'BPEOF'
# Source .bashrc for login shells
if [ -f ~/.bashrc ]; then
    . ~/.bashrc
fi
BPEOF
chown {user}:{user} {home}/.bash_profile && chmod 644 {home}/.bash_profile"#,
    );
    ssh_root_as_async(ip, key, &bash_profile, ssh_user).await?;

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
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale down
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale version

# Journal access - openclaw logs only
{user} ALL=(ALL) NOPASSWD: /usr/bin/journalctl -u openclaw --no-pager
{user} ALL=(ALL) NOPASSWD: /usr/bin/journalctl -u openclaw -n 200 --no-pager
SUDOEOF
chmod 440 /etc/sudoers.d/{user} && chown root:root /etc/sudoers.d/{user}
visudo -cf /etc/sudoers.d/{user}"#,
    );
    ssh_root_as_async(ip, key, &sudoers, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "sudoers".into(),
            message: e.to_string(),
        })?;

    // Setup .ssh/authorized_keys with deploy key via SCP to avoid shell interpolation.
    let scp_user = if ssh_user == "root" { "root" } else { ssh_user };
    let key_owned = key.to_path_buf();
    let ip_owned = ip.to_string();
    let scp_user_owned = scp_user.to_string();
    let authorized_keys = format!("{public_key_openssh}\n").into_bytes();
    tokio::task::spawn_blocking(move || {
        ssh::scp_upload_bytes(
            &ip_owned,
            &key_owned,
            &authorized_keys,
            "/tmp/.authorized_keys_upload",
            0o600,
            &scp_user_owned,
        )
    })
    .await
    .map_err(|e| AppError::Ssh(format!("spawn_blocking join: {e}")))??;

    let ssh_setup = format!(
        r#"mkdir -p {home}/.ssh && \
chmod 700 {home}/.ssh && \
mv /tmp/.authorized_keys_upload {home}/.ssh/authorized_keys && \
chmod 600 {home}/.ssh/authorized_keys && \
chown -R {user}:{user} {home}/.ssh"#,
    );
    ssh_root_as_async(ip, key, &ssh_setup, ssh_user).await?;

    // Enable lingering for systemd user services
    ssh_root_as_async(ip, key, &format!("loginctl enable-linger {user}"), ssh_user).await?;

    // Create runtime directory
    let runtime_dir = format!(
        r#"OPENCLAW_UID=$(id -u {user}) && \
mkdir -p /run/user/$OPENCLAW_UID && \
chown {user}:{user} /run/user/$OPENCLAW_UID && \
chmod 700 /run/user/$OPENCLAW_UID"#,
    );
    ssh_root_as_async(ip, key, &runtime_dir, ssh_user).await?;

    // Ensure the user systemd manager is started now, not only on next login/reboot.
    let user_manager = format!(
        r#"OPENCLAW_UID=$(id -u {user}) && \
systemctl start user@$OPENCLAW_UID.service >/dev/null 2>&1 || true && \
for i in $(seq 1 20); do \
  [ -S /run/user/$OPENCLAW_UID/bus ] && exit 0; \
  sleep 1; \
done; \
exit 0"#,
    );
    ssh_root_as_async(ip, key, &user_manager, ssh_user).await?;

    // If a backup was restored to /root/.openclaw, move it into the openclaw home.
    // Also fix any hardcoded /root/ paths in openclaw.json (workspace, plugin install paths, etc.)
    let restore_backup = format!(
        r#"if [ -d /root/.openclaw ]; then \
mkdir -p {home}/.openclaw && \
cp -a /root/.openclaw/. {home}/.openclaw/ && \
chown -R {user}:{user} {home}/.openclaw && \
chmod 700 {home}/.openclaw && \
rm -rf /root/.openclaw; \
fi && \
if [ -f {home}/.openclaw/openclaw.json ]; then \
sed -i 's|/root/.openclaw|{home}/.openclaw|g' {home}/.openclaw/openclaw.json && \
sed -i 's|/root/|{home}/|g' {home}/.openclaw/openclaw.json && \
chown {user}:{user} {home}/.openclaw/openclaw.json; \
fi"#,
    );
    ssh_root_as_async(ip, key, &restore_backup, ssh_user).await?;

    Ok(())
}
