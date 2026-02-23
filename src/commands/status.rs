use crate::digitalocean::DoClient;
use anyhow::Result;
use console::style;

/// List deployed openclaw-tagged droplets with IPs and status.
pub async fn run(do_token: &str) -> Result<()> {
    let client = DoClient::new(do_token)?;

    println!("Fetching openclaw droplets...\n");
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
