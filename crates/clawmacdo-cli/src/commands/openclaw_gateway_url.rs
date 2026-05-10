use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_with_user_async, ssh_root_as_async,
};
use serde_json::Value;
use std::path::PathBuf;

pub struct OpenclawGatewayUrlParams {
    pub instance: String,
    pub json: bool,
}

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
        Some("azure") => "azureuser",
        _ => "root",
    }
}

fn clean_instance(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--instance cannot be empty.");
    }
    if value.len() > 255 {
        bail!("--instance must be 255 bytes or fewer.");
    }
    if value.chars().any(char::is_control) {
        bail!("--instance cannot contain control characters.");
    }
    Ok(value.to_string())
}

fn parse_funnel_url(status: &str) -> Option<String> {
    status.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with("https://") {
            return None;
        }
        let url = trimmed.trim_end_matches(':');
        let url = if let Some(idx) = url.find(" (") {
            &url[..idx]
        } else {
            url
        };
        Some(url.to_string())
    })
}

fn parse_tailscale_dns_url(status_json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(status_json).ok()?;
    let dns_name = value.get("Self")?.get("DNSName")?.as_str()?.trim();
    if dns_name.is_empty() {
        return None;
    }
    Some(format!("https://{}", dns_name.trim_end_matches('.')))
}

fn build_token_cmd() -> String {
    let home = config::OPENCLAW_HOME;
    r#"export HOME="__HOME__"
node <<'NODE'
const fs = require('fs');
const path = require('path');
const configPath = path.join(process.env.HOME || '__HOME__', '.openclaw', 'openclaw.json');
try {
  const cfg = JSON.parse(fs.readFileSync(configPath, 'utf8'));
  process.stdout.write((cfg.gateway && cfg.gateway.auth && cfg.gateway.auth.token) || '');
} catch (_) {}
NODE
"#
    .replace("__HOME__", home)
}

pub async fn run(params: OpenclawGatewayUrlParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let (ip, key, provider) = find_deploy_record(&instance)?;
    let ssh_user = ssh_user_for_provider(&provider);

    let status = ssh_root_as_async(&ip, &key, "tailscale funnel status 2>&1 || true", ssh_user)
        .await
        .unwrap_or_default();
    let mut public_url = parse_funnel_url(&status);
    if public_url.is_none() {
        let status_json = ssh_root_as_async(
            &ip,
            &key,
            "tailscale status --json 2>/dev/null || true",
            ssh_user,
        )
        .await
        .unwrap_or_default();
        public_url = parse_tailscale_dns_url(&status_json);
    }

    let token = ssh_as_openclaw_with_user_async(&ip, &key, &build_token_cmd(), ssh_user)
        .await
        .unwrap_or_default()
        .trim()
        .to_string();

    let gateway_url = public_url.as_ref().map(|url| {
        if token.is_empty() {
            url.clone()
        } else {
            format!("{url}/auth.html#{token}")
        }
    });

    if params.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": gateway_url.is_some(),
                "instance": instance,
                "ip": ip,
                "public_url": public_url,
                "gateway_url": gateway_url,
                "gateway_token_present": !token.is_empty()
            }))?
        );
        return Ok(());
    }

    match gateway_url {
        Some(url) => {
            println!("{url}");
            Ok(())
        }
        None => {
            bail!("No Tailscale Funnel Gateway URL found for {instance}. Enable Funnel first.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_funnel_url_extracts_https_url() {
        let status = "https://example.ts.net:\n|-- / proxy http://127.0.0.1:18789";
        assert_eq!(
            parse_funnel_url(status).as_deref(),
            Some("https://example.ts.net")
        );
    }

    #[test]
    fn parse_tailscale_dns_url_uses_self_dns_name() {
        let status = r#"{"Self":{"DNSName":"node.tailnet.ts.net."}}"#;
        assert_eq!(
            parse_tailscale_dns_url(status).as_deref(),
            Some("https://node.tailnet.ts.net")
        );
    }
}
