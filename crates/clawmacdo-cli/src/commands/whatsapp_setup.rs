use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_async;
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

/// Enable WhatsApp channel on a deployed instance, set the phone number in .env,
/// enable the whatsapp plugin, restart the gateway, and fetch the pairing QR code.
pub async fn setup(query: &str, phone_number: &str) -> Result<()> {
    let (ip, key, _provider) = find_deploy_record(query)?;
    let home = config::OPENCLAW_HOME;

    println!("Setting up WhatsApp on {ip}...");

    // Step 1: Set WHATSAPP_PHONE_NUMBER in .env
    println!("[1/4] Setting WHATSAPP_PHONE_NUMBER in .env...");
    let set_phone_cmd = format!(
        "if grep -q '^WHATSAPP_PHONE_NUMBER=' {home}/.openclaw/.env 2>/dev/null; then \
           sed -i 's|^WHATSAPP_PHONE_NUMBER=.*|WHATSAPP_PHONE_NUMBER={phone_number}|' {home}/.openclaw/.env; \
         else \
           echo 'WHATSAPP_PHONE_NUMBER={phone_number}' >> {home}/.openclaw/.env; \
         fi && chmod 600 {home}/.openclaw/.env",
    );
    ssh_as_openclaw_async(&ip, &key, &set_phone_cmd).await?;

    // Step 2: Enable whatsapp plugin
    println!("[2/4] Enabling WhatsApp plugin...");
    let enable_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         (openclaw plugins enable whatsapp 2>&1 || true)",
    );
    let enable_out = ssh_as_openclaw_async(&ip, &key, &enable_cmd).await?;
    if !enable_out.trim().is_empty() {
        println!("  {}", enable_out.trim());
    }

    // Step 3: Restart gateway
    println!("[3/4] Restarting gateway service...");
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)";
    let restart_out = ssh_as_openclaw_async(&ip, &key, restart_cmd).await?;
    println!("  {}", restart_out.trim());

    // Step 4: Fetch WhatsApp QR code for pairing
    println!("[4/4] Fetching WhatsApp pairing QR code...");
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
    let qr_out = ssh_as_openclaw_async(&ip, &key, &qr_cmd).await?;
    println!("\n{}", qr_out.trim());

    println!("\nWhatsApp setup complete. Scan the QR code above with your WhatsApp app.");
    println!("If the QR code expired, run: clawmacdo whatsapp-qr --instance {query}");

    Ok(())
}

/// Fetch the WhatsApp pairing QR code from a deployed instance.
pub async fn fetch_qr(query: &str) -> Result<()> {
    let (ip, key, _provider) = find_deploy_record(query)?;
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
    let qr_out = ssh_as_openclaw_async(&ip, &key, &qr_cmd).await?;

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
