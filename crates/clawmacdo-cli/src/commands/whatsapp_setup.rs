use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_with_user_async, ssh_as_openclaw_with_user_multi_async,
};
use serde::Deserialize;
use std::path::PathBuf;

/// Build a shell command that fetches the WhatsApp QR code using a
/// background process + polling approach.  The openclaw login process
/// prints the QR and then keeps running (waiting for the scan), so a
/// synchronous `timeout 45s` blocks until the full timeout expires.
/// Instead we:
///   1. Kill any lingering login process.
///   2. Launch openclaw in the background with stdout/stderr piped to a
///      temp file via `stdbuf -oL` (line-buffered) so output flushes
///      immediately without needing a PTY.
///   3. Poll the file every 0.5 s for up to 40 iterations (≈ 20 s).
///      As soon as ≥ 500 bytes appear the QR block is likely present;
///      we also stop early if the file stops growing (openclaw exited).
///   4. Cat the file and return immediately.
///
/// The background openclaw process stays alive (up to 90 s) so the
/// WhatsApp linking handshake can complete after the user scans.
fn qr_fetch_shell_cmd(home: &str) -> String {
    format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\"; \
         export HOME=\"{home}\"; \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus; \
         pkill -f 'openclaw channels login' 2>/dev/null || true; sleep 0.3; \
         QF=/tmp/wa_qr_$$.txt; rm -f \"$QF\"; touch \"$QF\"; \
         TERM=dumb NO_COLOR=1 FORCE_COLOR=0 nohup stdbuf -oL timeout 90s openclaw channels login --channel whatsapp >\"$QF\" 2>&1 & \
         PREV=0; SAME=0; \
         for I in $(seq 1 40); do sleep 0.5; SZ=$(wc -c <\"$QF\" 2>/dev/null || echo 0); \
           if [ \"$SZ\" -ge 500 ]; then break; fi; \
           if [ \"$SZ\" -gt 0 ] && [ \"$SZ\" -eq \"$PREV\" ]; then SAME=$((SAME+1)); if [ \"$SAME\" -ge 6 ]; then break; fi; else SAME=0; fi; \
           PREV=$SZ; done; \
         sleep 0.3; cat \"$QF\" 2>/dev/null || echo 'QR not ready'",
    )
}

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
pub async fn setup(query: &str, phone_number: &str, reset: bool) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    println!("Setting up WhatsApp on {ip}...");

    let mut cmds: Vec<String> = Vec::new();
    let mut step = 0;
    let total_steps = if reset { 5 } else { 4 };

    if reset {
        step += 1;
        println!("[{step}/{total_steps}] Clearing WhatsApp session credentials...");
        cmds.push(format!(
            "rm -rf {home}/.openclaw/credentials/whatsapp && \
             echo 'WhatsApp session cleared'"
        ));
    }

    step += 1;
    println!("[{step}/{total_steps}] Setting WHATSAPP_PHONE_NUMBER in .env...");
    cmds.push(format!(
        "if grep -q '^WHATSAPP_PHONE_NUMBER=' {home}/.openclaw/.env 2>/dev/null; then \
           sed -i 's|^WHATSAPP_PHONE_NUMBER=.*|WHATSAPP_PHONE_NUMBER={phone_number}|' {home}/.openclaw/.env; \
         else \
           echo 'WHATSAPP_PHONE_NUMBER={phone_number}' >> {home}/.openclaw/.env; \
         fi && chmod 600 {home}/.openclaw/.env",
    ));

    step += 1;
    println!("[{step}/{total_steps}] Enabling WhatsApp plugin...");
    cmds.push(format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         (openclaw plugins enable whatsapp 2>&1 || true)",
    ));

    step += 1;
    println!("[{step}/{total_steps}] Restarting gateway service...");
    cmds.push(
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)"
            .to_string(),
    );

    step += 1;
    println!("[{step}/{total_steps}] Fetching WhatsApp pairing QR code...");
    cmds.push(qr_fetch_shell_cmd(home));

    let outputs = ssh_as_openclaw_with_user_multi_async(&ip, &key, cmds, ssh_user).await?;

    // Print relevant output
    let qr_idx = outputs.len() - 1;
    let gateway_idx = qr_idx - 1;
    for (i, out) in outputs.iter().enumerate() {
        let trimmed = out.trim();
        if i == qr_idx {
            // QR code gets its own section
            println!("\n{trimmed}");
        } else if !trimmed.is_empty() {
            println!("  {trimmed}");
        }
    }

    println!("\nWhatsApp setup complete. Scan the QR code above with your WhatsApp app.");
    println!("If the QR code expired, run: clawmacdo whatsapp-qr --instance {query}");
    let _ = gateway_idx;

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

    let qr_cmd = qr_fetch_shell_cmd(home);
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

// ── WhatsApp status via creds.json ───────────────────────────────────

/// Parsed WhatsApp pairing status from `creds.json`.
#[derive(Debug, Deserialize)]
pub struct WhatsAppCredsStatus {
    pub status: String,
    pub jid: Option<String>,
    pub name: Option<String>,
    pub registered: Option<bool>,
}

/// Build a shell command that reads WhatsApp pairing status from
/// `credentials/whatsapp/default/creds.json`.
///
/// Logic:
///   - File missing                                          → `not_paired`
///   - `me.id` present AND `account.accountSignature` exists → `connected`
///   - `me.id` present BUT no `accountSignature`             → `pending`
///   - `me.id` missing/empty                                 → `not_paired`
///
/// Note: the `registered` field is unreliable — it can be `false` even
/// after a successful device link.  The presence of `accountSignature`
/// is the real proof that the WhatsApp handshake completed.
fn status_shell_cmd(home: &str) -> String {
    format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\"; \
         export HOME=\"{home}\"; \
         CREDS=\"{home}/.openclaw/credentials/whatsapp/default/creds.json\"; \
         node -e \"\
           const fs=require('fs');\
           try{{\
             const c=JSON.parse(fs.readFileSync('$CREDS','utf8'));\
             const jid=(c.me&&c.me.id)||'';\
             const name=(c.me&&c.me.name)||'';\
             const sig=(c.account&&c.account.accountSignature)||'';\
             if(!jid){{console.log(JSON.stringify({{status:'not_paired'}}))}}\
             else if(sig){{console.log(JSON.stringify({{status:'connected',jid:jid,name:name,registered:true}}))}}\
             else{{console.log(JSON.stringify({{status:'pending',jid:jid,name:name,registered:false}}))}}\
           }}catch(e){{console.log(JSON.stringify({{status:'not_paired'}}))}}\
         \" 2>/dev/null || echo '{{\"status\":\"not_paired\"}}'"
    )
}

/// Query the WhatsApp channel status on a deployed instance.
pub async fn status(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    let cmd = status_shell_cmd(home);
    let out = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let trimmed = out.trim();

    match serde_json::from_str::<WhatsAppCredsStatus>(trimmed) {
        Ok(s) => {
            println!("WhatsApp status: {}", s.status);
            if let Some(jid) = &s.jid {
                println!("  JID:        {jid}");
            }
            if let Some(name) = &s.name {
                println!("  Name:       {name}");
            }
            if let Some(reg) = s.registered {
                println!("  Registered: {reg}");
            }
        }
        Err(_) => {
            println!("Could not parse credentials output:\n{trimmed}");
        }
    }

    Ok(())
}

/// Poll the WhatsApp credentials until the status reaches "connected".
pub async fn wait_for_scan(query: &str, timeout_secs: u64) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;
    let cmd = status_shell_cmd(home);

    println!("Waiting for WhatsApp scan on {ip} (timeout {timeout_secs}s)...");

    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_secs(3);
    let deadline = std::time::Duration::from_secs(timeout_secs);

    loop {
        let out = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
        let trimmed = out.trim();

        if let Ok(s) = serde_json::from_str::<WhatsAppCredsStatus>(trimmed) {
            match s.status.as_str() {
                "connected" => {
                    println!("\rConnected!                    ");
                    if let Some(jid) = &s.jid {
                        println!("  JID:        {jid}");
                    }
                    if let Some(name) = &s.name {
                        println!("  Name:       {name}");
                    }
                    return Ok(());
                }
                other => {
                    let elapsed = start.elapsed().as_secs();
                    print!("\r  status: {other} ({elapsed}s elapsed)");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
            }
        }

        if start.elapsed() >= deadline {
            println!();
            bail!(
                "Timed out after {timeout_secs}s waiting for WhatsApp to connect. \
                 Current status: not connected."
            );
        }

        tokio::time::sleep(poll_interval).await;
    }
}
