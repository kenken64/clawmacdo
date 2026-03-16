use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::{ssh_as_openclaw_async, ssh_root_async};
use std::path::{Path, PathBuf};

/// Look up a deploy record by hostname, IP, or deploy ID.
/// Returns (ip, ssh_key_path, provider).
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

/// Set up Tailscale Funnel on a deployed OpenClaw instance.
///
/// Steps:
/// 1. Install Tailscale (if not already installed)
/// 2. Connect Tailscale with the provided auth key
/// 3. Enable Funnel on port 18789
/// 4. Retrieve and display the public Funnel URL
/// 5. Update openclaw.json controlUi.allowedOrigins + trustedProxies
/// 6. Auto-approve all pending devices
pub async fn setup(query: &str, auth_key: &str, port: u16) -> Result<()> {
    let (ip, key, _provider) = find_deploy_record(query)?;
    let home = config::OPENCLAW_HOME;

    println!("Setting up Tailscale Funnel on {ip}...\n");

    // Step 1: Install Tailscale if not present
    println!("[1/6] Installing Tailscale...");
    let install_cmd = r#"
if command -v tailscale >/dev/null 2>&1; then
    echo "ALREADY_INSTALLED"
else
    curl -fsSL "https://pkgs.tailscale.com/stable/ubuntu/noble.noarmor.gpg" | \
        tee /usr/share/keyrings/tailscale-archive-keyring.gpg > /dev/null && \
    curl -fsSL "https://pkgs.tailscale.com/stable/ubuntu/noble.tailscale-keyring.list" | \
        tee /etc/apt/sources.list.d/tailscale.list > /dev/null && \
    apt-get update -qq && apt-get install -y -qq tailscale && \
    systemctl enable tailscaled && systemctl start tailscaled && \
    ufw allow 41641/udp comment 'Tailscale' && \
    echo "INSTALLED"
fi
"#;
    let install_out = ssh_root_async(&ip, &key, install_cmd).await?;
    let trimmed = install_out.trim();
    if trimmed.contains("ALREADY_INSTALLED") {
        println!("  Tailscale already installed.");
    } else {
        println!("  Tailscale installed successfully.");
    }

    // Step 2: Connect Tailscale with auth key
    println!("[2/6] Connecting Tailscale...");
    let auth_key_escaped = auth_key.replace('\'', "'\\''");
    let connect_cmd = format!(
        "tailscale status --json 2>/dev/null | grep -q '\"BackendState\": \"Running\"' && \
         echo 'ALREADY_CONNECTED' || \
         (tailscale up --auth-key '{auth_key_escaped}' --hostname $(hostname -s) 2>&1 && echo 'CONNECTED')"
    );
    let connect_out = ssh_root_async(&ip, &key, &connect_cmd).await?;
    let connect_trimmed = connect_out.trim();
    if connect_trimmed.contains("ALREADY_CONNECTED") {
        println!("  Tailscale already connected.");
    } else if connect_trimmed.contains("CONNECTED") {
        println!("  Tailscale connected.");
    } else {
        println!("  Tailscale connect output: {connect_trimmed}");
    }

    // Step 3: Enable Funnel on the specified port
    println!("[3/6] Enabling Tailscale Funnel on port {port}...");
    let funnel_cmd = format!("tailscale funnel --bg {port} 2>&1");
    let funnel_out = ssh_root_async(&ip, &key, &funnel_cmd).await?;
    let funnel_trimmed = funnel_out.trim();
    if !funnel_trimmed.is_empty() {
        println!("  {funnel_trimmed}");
    }

    // Step 4: Get the Funnel public URL
    println!("[4/6] Retrieving Funnel public URL...");
    let status_cmd = "tailscale funnel status 2>&1";
    let status_out = ssh_root_async(&ip, &key, status_cmd).await?;
    let status_trimmed = status_out.trim();
    println!("  {}", status_trimmed.replace('\n', "\n  "));

    // Extract the https:// URL from funnel status output
    let funnel_url = status_trimmed.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with("https://") {
            // Remove trailing colon and annotations like " (funnel on):"
            let url = trimmed.trim_end_matches(':');
            let url = if let Some(idx) = url.find(" (") {
                &url[..idx]
            } else {
                url
            };
            Some(url.to_string())
        } else {
            None
        }
    });

    let funnel_url = match funnel_url {
        Some(url) => {
            println!("\n  Public URL: {url}");
            url
        }
        None => {
            // Fallback: construct from tailscale DNS name
            let dns_cmd = "tailscale status --json 2>/dev/null | grep -o '\"DNSName\":\"[^\"]*\"' | head -1 | cut -d'\"' -f4";
            let dns_out = ssh_root_async(&ip, &key, dns_cmd).await?;
            let dns_name = dns_out.trim().trim_end_matches('.');
            if dns_name.is_empty() {
                bail!("Could not determine Tailscale Funnel URL. Check `tailscale funnel status` on the instance.");
            }
            let url = format!("https://{dns_name}");
            println!("\n  Public URL: {url}");
            url
        }
    };

    // Step 5: Update openclaw.json with controlUi.allowedOrigins, trustedProxies, and read auth token
    println!("[5/6] Updating openclaw.json (allowedOrigins + trustedProxies)...");
    let config_cmd = format!(
        r#"export PATH="{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin" && \
         export HOME="{home}" && \
         CONFIG="{home}/.openclaw/openclaw.json" && \
         if [ -f "$CONFIG" ]; then \
           if command -v node >/dev/null 2>&1; then \
             node -e "
               const fs = require('fs');
               const cfg = JSON.parse(fs.readFileSync('$CONFIG', 'utf8'));
               if (!cfg.gateway) cfg.gateway = {{}};
               // controlUi.allowedOrigins
               if (!cfg.gateway.controlUi) cfg.gateway.controlUi = {{}};
               const origins = cfg.gateway.controlUi.allowedOrigins || [];
               const url = '{funnel_url}';
               if (!origins.includes(url) && !origins.includes('*')) {{
                 origins.push(url);
               }}
               cfg.gateway.controlUi.allowedOrigins = origins;
               // Disable device pairing for Control UI via Funnel
               cfg.gateway.controlUi.dangerouslyDisableDeviceAuth = true;
               // trustedProxies — trust loopback (Tailscale Funnel proxies via 127.0.0.1)
               if (!cfg.gateway.trustedProxies) {{
                 cfg.gateway.trustedProxies = ['127.0.0.1/8', '::1/128'];
               }}
               fs.writeFileSync('$CONFIG', JSON.stringify(cfg, null, 2) + '\n');
               const token = (cfg.gateway && cfg.gateway.auth && cfg.gateway.auth.token) || '';
               console.log('ORIGINS=' + JSON.stringify(origins));
               console.log('PROXIES=' + JSON.stringify(cfg.gateway.trustedProxies));
               console.log('AUTH_TOKEN=' + token);
             "; \
           else \
             echo "node not found — please manually add \"{funnel_url}\" to controlUi.allowedOrigins in openclaw.json"; \
           fi; \
         else \
           echo "openclaw.json not found at $CONFIG"; \
         fi"#,
    );
    let config_out = ssh_as_openclaw_async(&ip, &key, &config_cmd).await?;
    let config_trimmed = config_out.trim();

    // Parse auth token from output
    let auth_token = config_trimmed
        .lines()
        .find_map(|line| line.strip_prefix("AUTH_TOKEN="))
        .unwrap_or("")
        .to_string();

    // Show config updates
    for line in config_trimmed.lines() {
        if let Some(origins) = line.strip_prefix("ORIGINS=") {
            println!("  allowedOrigins: {origins}");
        } else if let Some(proxies) = line.strip_prefix("PROXIES=") {
            println!("  trustedProxies: {proxies}");
        }
    }

    // Restart gateway to pick up the config change
    println!("\nRestarting OpenClaw gateway...");
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)";
    let restart_out = ssh_as_openclaw_async(&ip, &key, restart_cmd).await?;
    println!("  {}", restart_out.trim());

    // Step 6: Auto-approve all pending devices
    println!("[6/6] Approving all pending devices...");
    let approved = approve_pending_devices(&ip, &key, home).await?;
    if approved == 0 {
        println!("  No pending devices found.");
    } else {
        println!("  Approved {approved} device(s).");
    }

    println!("\nTailscale Funnel setup complete!");
    println!("Public URL:      {funnel_url}");
    if !auth_token.is_empty() {
        println!("Gateway Token:   {auth_token}");
        println!("\nTo connect: open the URL above, click Settings (gear icon),");
        println!("paste the Gateway Token, and save.");
    } else {
        println!("Gateway Token:   (not found — check openclaw.json)");
    }

    println!("\nNote: If you connect from a new browser, approve it with:");
    println!("  clawmacdo device-approve --instance {query}");

    Ok(())
}

/// Approve all pending OpenClaw devices on a deployed instance.
/// Moves entries from devices/pending.json to devices/paired.json directly.
/// Returns the number of devices approved.
async fn approve_pending_devices(ip: &str, key: &Path, home: &str) -> Result<u32> {
    let cmd = format!(
        r#"export HOME="{home}" && node -e "
const fs=require('fs');
const pf='{home}/.openclaw/devices/pending.json';
const af='{home}/.openclaw/devices/paired.json';
let pending={{}};try{{pending=JSON.parse(fs.readFileSync(pf,'utf8'))}}catch(e){{}}
let paired={{}};try{{paired=JSON.parse(fs.readFileSync(af,'utf8'))}}catch(e){{}}
const keys=Object.keys(pending);
for(const k of keys){{paired[k]=pending[k];console.log('Approved '+k)}}
if(keys.length>0){{
  fs.writeFileSync(af,JSON.stringify(paired,null,2)+'\n');
  fs.writeFileSync(pf,'{{}}\n');
}}
console.log('APPROVED='+keys.length);
""#,
    );
    let output = ssh_as_openclaw_async(ip, key, &cmd).await?;
    let trimmed = output.trim();

    for line in trimmed.lines() {
        if line.starts_with("Approved") {
            println!("  {line}");
        }
    }

    let count = trimmed
        .lines()
        .find_map(|line| {
            line.strip_prefix("APPROVED=")
                .and_then(|n| n.parse::<u32>().ok())
        })
        .unwrap_or(0);

    Ok(count)
}

/// Standalone command: approve all pending devices on a deployed instance.
pub async fn device_approve_all(query: &str) -> Result<()> {
    let (ip, key, _provider) = find_deploy_record(query)?;
    let home = config::OPENCLAW_HOME;

    println!("Approving all pending devices on {ip}...\n");

    let approved = approve_pending_devices(&ip, &key, home).await?;
    if approved == 0 {
        println!("No pending devices found.");
    } else {
        println!("\nApproved {approved} device(s).");
    }

    Ok(())
}

/// Turn Tailscale Funnel ON for a deployed instance.
pub async fn funnel_on(query: &str, port: u16) -> Result<()> {
    let (ip, key, _provider) = find_deploy_record(query)?;

    println!("Enabling Tailscale Funnel on {ip} (port {port})...\n");

    let cmd = format!("tailscale funnel --bg {port} 2>&1");
    let out = ssh_root_async(&ip, &key, &cmd).await?;
    let trimmed = out.trim();
    if !trimmed.is_empty() {
        println!("  {trimmed}");
    }

    // Show status
    let status_out = ssh_root_async(&ip, &key, "tailscale funnel status 2>&1").await?;
    println!("{}", status_out.trim());

    println!("\nFunnel enabled.");
    Ok(())
}

/// Turn Tailscale Funnel OFF for a deployed instance.
pub async fn funnel_off(query: &str) -> Result<()> {
    let (ip, key, _provider) = find_deploy_record(query)?;

    println!("Disabling Tailscale Funnel on {ip}...\n");

    let cmd = "tailscale funnel off 2>&1";
    let out = ssh_root_async(&ip, &key, cmd).await?;
    let trimmed = out.trim();
    if !trimmed.is_empty() {
        println!("  {trimmed}");
    }

    println!("Funnel disabled.");
    Ok(())
}

/// Toggle Tailscale Funnel on/off. Used by the web UI API handler.
/// Returns (ok, message, funnel_url, gateway_token).
pub async fn funnel_toggle(
    query: &str,
    action: &str,
    port: u16,
) -> Result<(bool, String, Option<String>, Option<String>)> {
    let (ip, key, _provider) = find_deploy_record(query)?;

    match action {
        "on" => {
            let cmd = format!("tailscale funnel --bg {port} 2>&1");
            ssh_root_async(&ip, &key, &cmd).await?;

            // Get the funnel URL
            let status_out = ssh_root_async(&ip, &key, "tailscale funnel status 2>&1").await?;
            let funnel_url = status_out.lines().find_map(|line| {
                let t = line.trim();
                if t.starts_with("https://") {
                    let url = t.trim_end_matches(':');
                    let url = if let Some(idx) = url.find(" (") {
                        &url[..idx]
                    } else {
                        url
                    };
                    Some(url.to_string())
                } else {
                    None
                }
            });

            let url = match funnel_url {
                Some(u) => u,
                None => {
                    let dns_cmd = "tailscale status --json 2>/dev/null | grep -o '\"DNSName\":\"[^\"]*\"' | head -1 | cut -d'\"' -f4";
                    let dns_out = ssh_root_async(&ip, &key, dns_cmd).await?;
                    let dns_name = dns_out.trim().trim_end_matches('.');
                    if dns_name.is_empty() {
                        return Ok((
                            true,
                            "Funnel enabled but could not determine URL.".into(),
                            None,
                            None,
                        ));
                    }
                    format!("https://{dns_name}")
                }
            };

            // Update openclaw.json: add allowedOrigins + trustedProxies, read auth token
            let home = config::OPENCLAW_HOME;
            let config_cmd = format!(
                r#"export PATH="{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin" && \
                 export HOME="{home}" && \
                 CONFIG="{home}/.openclaw/openclaw.json" && \
                 if [ -f "$CONFIG" ] && command -v node >/dev/null 2>&1; then \
                   node -e "
                     const fs = require('fs');
                     const cfg = JSON.parse(fs.readFileSync('$CONFIG', 'utf8'));
                     if (!cfg.gateway) cfg.gateway = {{}};
                     if (!cfg.gateway.controlUi) cfg.gateway.controlUi = {{}};
                     const origins = cfg.gateway.controlUi.allowedOrigins || [];
                     const url = '{url}';
                     if (!origins.includes(url) && !origins.includes('*')) {{
                       origins.push(url);
                     }}
                     cfg.gateway.controlUi.allowedOrigins = origins;
                     // Disable device pairing for Control UI via Funnel
                     cfg.gateway.controlUi.dangerouslyDisableDeviceAuth = true;
                     if (!cfg.gateway.trustedProxies) {{
                       cfg.gateway.trustedProxies = ['127.0.0.1/8', '::1/128'];
                     }}
                     fs.writeFileSync('$CONFIG', JSON.stringify(cfg, null, 2) + '\n');
                     const token = (cfg.gateway && cfg.gateway.auth && cfg.gateway.auth.token) || '';
                     console.log('AUTH_TOKEN=' + token);
                   "; \
                 else \
                   echo 'AUTH_TOKEN='; \
                 fi"#,
            );
            let config_out = ssh_as_openclaw_async(&ip, &key, &config_cmd)
                .await
                .unwrap_or_default();

            let token = config_out
                .lines()
                .find_map(|line| line.strip_prefix("AUTH_TOKEN="))
                .unwrap_or("")
                .trim()
                .to_string();

            // Deploy auth.html + auth.js to control-ui dir for one-click token auth
            // Uses external JS file to satisfy Content-Security-Policy (script-src 'self')
            let deploy_auth_cmd = format!(
                r#"export PATH="{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin" && \
                 export HOME="{home}" && \
                 CTRL_UI=$(find {home}/.local/share/pnpm -path '*/openclaw/dist/control-ui/index.html' 2>/dev/null | head -1 | xargs dirname 2>/dev/null) && \
                 if [ -n "$CTRL_UI" ]; then \
                   cat > "$CTRL_UI/assets/auth.js" << 'JSEOF'
(function(){{
  var p=document.querySelector('p');
  var t=location.hash.slice(1);
  if(!t){{p.textContent='No token provided.';return}}
  p.textContent='Connecting to gateway...';
  setTimeout(function(){{location.replace('/#token='+t);}},2000);
}})();
JSEOF
                   cat > "$CTRL_UI/auth.html" << 'HTMLEOF'
<!DOCTYPE html><html><head><meta charset="UTF-8"><title>Connecting...</title>
<style>body{{background:#0f172a;color:#94a3b8;font-family:system-ui;display:flex;align-items:center;justify-content:center;height:100vh;margin:0}}.c{{text-align:center}}.s{{animation:spin 1s linear infinite;width:32px;height:32px;border:3px solid #334155;border-top-color:#3b82f6;border-radius:50%;margin:0 auto 16px}}@keyframes spin{{to{{transform:rotate(360deg)}}}}</style></head><body><div class="c"><div class="s"></div><p>Setting up gateway token...</p></div>
<script src="./assets/auth.js"></script></body></html>
HTMLEOF
                   echo 'AUTH_HTML_OK'; \
                 else \
                   echo 'AUTH_HTML_FAIL'; \
                 fi"#,
            );
            let _ = ssh_as_openclaw_async(&ip, &key, &deploy_auth_cmd).await;

            // Restart gateway to pick up config changes
            let restart_cmd =
                "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
                 (systemctl --user restart openclaw-gateway.service 2>/dev/null || true)";
            let _ = ssh_as_openclaw_async(&ip, &key, restart_cmd).await;

            // Build the one-click URL: /auth.html#<token> auto-sets sessionStorage and redirects
            let auth_url = if token.is_empty() {
                url.clone()
            } else {
                format!("{url}/auth.html#{token}")
            };

            let gw_token = if token.is_empty() { None } else { Some(token) };

            Ok((
                true,
                format!("Funnel enabled at {url}"),
                Some(auth_url),
                gw_token,
            ))
        }
        "off" => {
            ssh_root_async(&ip, &key, "tailscale funnel off 2>&1").await?;
            Ok((true, "Funnel disabled.".into(), None, None))
        }
        _ => Ok((
            false,
            format!("Unknown action: {action}. Use 'on' or 'off'."),
            None,
            None,
        )),
    }
}
