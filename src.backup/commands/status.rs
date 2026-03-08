use crate::digitalocean::DoClient;
use crate::tencent::TencentClient;
use anyhow::{bail, Result};
use console::style;

/// List deployed openclaw-tagged instances with IPs and status.
pub async fn run(
    provider: &str,
    do_token: &str,
    tencent_secret_id: &str,
    tencent_secret_key: &str,
) -> Result<()> {
    match provider {
        "digitalocean" => run_do(do_token).await,
        "tencent" => run_tencent(tencent_secret_id, tencent_secret_key).await,
        _ => bail!("Unknown provider '{provider}'. Use 'digitalocean' or 'tencent'."),
    }
}

async fn run_do(do_token: &str) -> Result<()> {
    let client = DoClient::new(do_token)?;

    println!("Fetching openclaw droplets (DigitalOcean)...\n");
    let droplets = client.list_droplets().await?;

    if droplets.is_empty() {
        println!("No droplets found with tag 'openclaw'.");
        return Ok(());
    }

    println!(
        "  {:<12}  {:<25}  {:<18}  {:<10}  {:<10}",
        "ID", "Name", "IP", "Region", "Status"
    );
    println!("  {}", "-".repeat(80));

    for d in &droplets {
        let ip = d.public_ip().unwrap_or_else(|| "N/A".into());
        let status_styled = match d.status.as_str() {
            "active" => style(&d.status).green().to_string(),
            "new" => style(&d.status).yellow().to_string(),
            _ => style(&d.status).red().to_string(),
        };
        println!(
            "  {:<12}  {:<25}  {:<18}  {:<10}  {:<10}",
            d.id, d.name, ip, d.region.slug, status_styled
        );
    }

    println!("\n  Total: {} droplet(s)", droplets.len());
    Ok(())
}

async fn run_tencent(secret_id: &str, secret_key: &str) -> Result<()> {
    let client = TencentClient::new(secret_id, secret_key, crate::config::DEFAULT_TENCENT_REGION)?;

    println!("Fetching openclaw instances (Tencent Cloud)...\n");
    let instances = client.list_openclaw_instances().await?;

    if instances.is_empty() {
        println!("No instances found with tag app=openclaw.");
        return Ok(());
    }

    println!(
        "  {:<22}  {:<25}  {:<18}  {:<12}",
        "Instance ID", "Name", "IP", "Status"
    );
    println!("  {}", "-".repeat(80));

    for inst in &instances {
        let ip = inst.public_ip.as_deref().unwrap_or("N/A");
        let status_styled = match inst.status.as_str() {
            "RUNNING" => style(&inst.status).green().to_string(),
            "PENDING" => style(&inst.status).yellow().to_string(),
            _ => style(&inst.status).red().to_string(),
        };
        println!(
            "  {:<22}  {:<25}  {:<18}  {:<12}",
            inst.id, inst.name, ip, status_styled
        );
    }

    println!("\n  Total: {} instance(s)", instances.len());
    Ok(())
}
