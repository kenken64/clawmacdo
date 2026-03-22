use anyhow::{bail, Context, Result};
#[cfg(feature = "lightsail")]
use clawmacdo_cloud::CloudProvider;
use clawmacdo_core::config::{self, CloudProviderType};
use clawmacdo_db as db;
use std::path::PathBuf;

/// Look up a deploy record by hostname, IP, or deploy ID.
fn find_deploy_record(query: &str) -> Result<(config::DeployRecord, PathBuf)> {
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
            return Ok((record, path));
        }
    }

    bail!("No deploy record found for '{query}'. Use a deploy ID, hostname, or IP address.");
}

pub async fn run(query: &str) -> Result<()> {
    let (record, record_path) = find_deploy_record(query)?;
    let old_ip = &record.ip_address;
    let hostname = &record.hostname;
    let provider = record
        .provider
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Deploy record has no provider set"))?;

    println!("Looking up current IP for '{hostname}' on {provider}...");

    let new_ip: String = match provider {
        #[cfg(feature = "lightsail")]
        CloudProviderType::Lightsail => {
            let ls =
                clawmacdo_cloud::lightsail_cli::LightsailCliProvider::new(record.region.clone());
            let instance = ls
                .wait_for_active(hostname, 5)
                .await
                .context("Failed to query Lightsail instance")?;
            instance
                .public_ip
                .ok_or_else(|| anyhow::anyhow!("Instance '{hostname}' has no public IP"))?
        }
        #[cfg(feature = "digitalocean")]
        CloudProviderType::DigitalOcean => {
            // DO requires a token — try from env
            let token =
                std::env::var("DO_TOKEN").context("Set DO_TOKEN env var to query DigitalOcean")?;
            let client = clawmacdo_cloud::digitalocean::DoClient::new(&token)?;
            let droplets = client
                .list_droplets()
                .await
                .context("Failed to list droplets")?;
            let droplet = droplets
                .iter()
                .find(|d| d.name == *hostname)
                .ok_or_else(|| anyhow::anyhow!("Droplet '{hostname}' not found"))?;
            droplet
                .public_ip()
                .ok_or_else(|| anyhow::anyhow!("Droplet '{hostname}' has no public IP"))?
        }
        #[cfg(feature = "byteplus")]
        CloudProviderType::BytePlus => {
            let ak =
                std::env::var("BYTEPLUS_ACCESS_KEY").context("Set BYTEPLUS_ACCESS_KEY env var")?;
            let sk =
                std::env::var("BYTEPLUS_SECRET_KEY").context("Set BYTEPLUS_SECRET_KEY env var")?;
            let client = clawmacdo_cloud::byteplus::BytePlusClient::new(&ak, &sk, &record.region)?;
            let instance_id = record
                .instance_id
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No instance_id in deploy record"))?;
            let instance = client
                .describe_instance(instance_id)
                .await
                .context("Failed to query BytePlus instance")?;
            instance
                .public_ip
                .ok_or_else(|| anyhow::anyhow!("Instance has no public IP"))?
        }
        _ => bail!("update-ip not supported for provider '{provider}'"),
    };

    if new_ip == *old_ip {
        println!("IP unchanged: {old_ip}");
        return Ok(());
    }

    println!("IP changed: {old_ip} -> {new_ip}");

    // Update JSON deploy record — read, replace IP, write back
    let mut raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&record_path)?)?;
    raw["ip_address"] = serde_json::Value::String(new_ip.clone());
    std::fs::write(&record_path, serde_json::to_string_pretty(&raw)?)?;
    println!("  Updated: {}", record_path.display());

    // Update SQLite
    if let Ok(conn) = db::init_db() {
        let _ = db::update_deployment_status(
            &conn,
            &record.id,
            "completed",
            Some(&new_ip),
            Some(hostname),
        );
        println!("  Updated: deployments.db");
    }

    println!("\nDeploy record updated. New IP: {new_ip}");
    Ok(())
}
