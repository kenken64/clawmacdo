use crate::config;
use crate::digitalocean::DoClient;
use anyhow::Result;
use dialoguer::Confirm;

pub struct DestroyParams {
    pub do_token: String,
    pub name: String,
}

pub async fn run(params: DestroyParams) -> Result<()> {
    let client = DoClient::new(&params.do_token)?;

    println!("Fetching openclaw droplets...");
    let droplets = client.list_droplets().await?;
    let droplet = droplets
        .into_iter()
        .find(|d| d.name == params.name)
        .ok_or_else(|| anyhow::anyhow!("No openclaw droplet found with name '{}'", params.name))?;

    let ip = droplet.public_ip().unwrap_or_else(|| "N/A".into());
    println!();
    println!("Droplet to destroy:");
    println!("  Name:   {}", droplet.name);
    println!("  IP:     {ip}");
    println!("  Region: {}", droplet.region.slug);

    let confirmed = Confirm::new()
        .with_prompt("Permanently destroy this droplet?")
        .default(false)
        .interact()?;
    if !confirmed {
        println!("Cancelled.");
        return Ok(());
    }

    println!(
        "\nDeleting droplet '{}' (ID {})...",
        droplet.name, droplet.id
    );
    client.delete_droplet(droplet.id).await?;
    println!("Droplet deleted.");

    let hostname_suffix = droplet
        .name
        .strip_prefix("openclaw-")
        .unwrap_or(&droplet.name);
    let expected_key_name = format!("clawmacdo-{hostname_suffix}");
    let keys = client.list_ssh_keys().await?;
    if let Some(key) = keys.into_iter().find(|k| k.name == expected_key_name) {
        println!(
            "Deleting SSH key '{}' (ID {}, fingerprint {})...",
            key.name, key.id, key.fingerprint
        );
        client.delete_ssh_key(key.id).await?;
        println!("SSH key deleted.");
    } else {
        println!("SSH key '{}' not found; skipping.", expected_key_name);
    }

    let local_key = config::keys_dir()?.join(format!("clawmacdo_{hostname_suffix}"));
    if local_key.exists() {
        std::fs::remove_file(&local_key)?;
        println!("Removed local key: {}", local_key.display());
    } else {
        println!("Local key not found; skipping: {}", local_key.display());
    }

    println!(
        "\nDestroy complete for '{}' ({ip}, {}).",
        droplet.name, droplet.region.slug
    );
    Ok(())
}
