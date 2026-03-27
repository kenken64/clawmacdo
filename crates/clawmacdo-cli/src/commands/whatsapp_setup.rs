use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_with_user_async, ssh_as_openclaw_with_user_multi_async,
};
use std::path::PathBuf;

/// Look up a deploy record by hostname, IP, or deploy ID.
fn find_deploy_record(query: &str) -> Result<(String, PathBuf, Option<String>)> {
    let deploys_dir = config::deploys_dir()?;
    if !deploys_dir.exists() {
        bail!("No deploy records found. Deploy an instance first.");
    }

    for entry in std::fs::read_dir(&deploys_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let contents = std::fs::read_to_string(&path)?;
        let record: config::DeployRecord = match serde_json::from_str(&contents) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if record.id == query || record.hostname == query || record.ip_address == query {
            let provider = record.provider.map(|p| p.to_string());
            return Ok((
                record.ip_address,
                PathBuf::from(record.ssh_key_path),
                provider,
            ));
        }
    }

    bail!("No deploy record found for '{query}'. Use a deploy ID, hostname, or IP address.");
}

fn ssh_user_for_provider(provider: &Option<String>) -> &'static str {
    match provider.as_deref() {
        Some("lightsail") => "ubuntu",
        _ => "root",
    }
}

/// Enable WhatsApp channel on a deployed instance, set the phone number in .env,
/// enable the whatsapp plugin, restart the gateway, and fetch the pairing QR code.
pub async fn setup(query: &str, phone_number: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Setting up WhatsApp on {ip}...");

    let set_phone_cmd = format!(
        "if grep -q '^WHATSAPP_PHONE_NUMBER=' {home}/.openclaw/.env 2>/dev/null; then \
           sed -i 's|^WHATSAPP_PHONE_NUMBER=.*|WHATSAPP_PHONE_NUMBER={phone_number}|' {home}/.openclaw/.env; \
         else \
           echo 'WHATSAPP_PHONE_NUMBER={phone_number}' >> {home}/.openclaw/.env; \
         fi && chmod 600 {home}/.openclaw/.env",
    );
    let enable_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         (openclaw plugins enable whatsapp 2>&1 || true)",
    );
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)";
    let qr_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if command -v timeout >/dev/null 2>&1; then \
           timeout 45s openclaw channels login --channel whatsapp 2>&1 || true; \
         else \
           openclaw channels login --channel whatsapp 2>&1 || true; \
         fi",
    );

    // All 4 steps share one SSH session — one TCP connect + handshake instead of four.
    println!("[1/4] Setting WHATSAPP_PHONE_NUMBER in .env...");
    println!("[2/4] Enabling WhatsApp plugin...");
    println!("[3/4] Restarting gateway service...");
    println!("[4/4] Fetching WhatsApp pairing QR code...");
    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![set_phone_cmd, enable_cmd, restart_cmd.to_string(), qr_cmd],
        ssh_user,
    )
    .await?;

    // outputs[0] = set_phone (discard)
    // outputs[1] = enable plugin
    if !outputs[1].trim().is_empty() {
        println!("  {}", outputs[1].trim());
    }
    // outputs[2] = restart gateway
    println!("  {}", outputs[2].trim());
    // outputs[3] = QR code
    println!("\n{}", outputs[3].trim());

    println!("\nWhatsApp setup complete. Scan the QR code above with your WhatsApp app.");
    println!("If the QR code expired, run: clawmacdo whatsapp-qr --instance {query}");

    Ok(())
}

/// Reset WhatsApp pairing state on a deployed instance.
/// Clears the WhatsApp session credentials and restarts the gateway,
/// forcing a fresh QR code pairing on next login.
pub async fn reset(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Resetting WhatsApp pairing on {ip}...");

    let reset_cmd = format!(
        "rm -rf {home}/.openclaw/credentials/whatsapp && \
         echo 'WhatsApp session cleared'"
    );
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)";

    println!("[1/2] Clearing WhatsApp session credentials...");
    println!("[2/2] Restarting gateway...");
    let outputs = ssh_as_openclaw_with_user_multi_async(
        &ip,
        &key,
        vec![reset_cmd, restart_cmd.to_string()],
        ssh_user,
    )
    .await?;

    println!("  {}", outputs[0].trim());
    println!("  {}", outputs[1].trim());

    println!("\nWhatsApp pairing reset. To re-pair, run:");
    println!("  clawmacdo whatsapp-qr --instance {query}");
    println!("Then scan the QR code with your WhatsApp app.");

    Ok(())
}

/// Fetch the WhatsApp pairing QR code from a deployed instance.
pub async fn fetch_qr(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Fetching WhatsApp QR code from {ip}...");

    let qr_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if command -v timeout >/dev/null 2>&1; then \
           timeout 45s openclaw channels login --channel whatsapp 2>&1 || true; \
         else \
           openclaw channels login --channel whatsapp 2>&1 || true; \
         fi",
    );
    let qr_out = ssh_as_openclaw_with_user_async(&ip, &key, &qr_cmd, ssh_user).await?;

    let lowered = qr_out.to_ascii_lowercase();
    if lowered.contains("unsupported channel: whatsapp")
        || lowered.contains("unsupported channel whatsapp")
    {
        println!("WhatsApp channel is not supported on this instance.");
        println!("Try: clawmacdo whatsapp-setup --instance {query} --phone-number <NUMBER>");
        return Ok(());
    }

    println!("\n{}", qr_out.trim());
    println!("\nScan the QR code above with your WhatsApp app.");

    Ok(())
}
