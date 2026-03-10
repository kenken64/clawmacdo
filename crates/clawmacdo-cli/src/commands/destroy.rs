use anyhow::{bail, Result};
use clawmacdo_cloud::digitalocean::DoClient;
use clawmacdo_cloud::tencent::TencentClient;
#[cfg(feature = "lightsail")]
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_core::config;
use dialoguer::Confirm;

pub struct DestroyParams {
    pub provider: String,
    pub do_token: String,
    pub tencent_secret_id: String,
    pub tencent_secret_key: String,
    pub name: String,
    pub yes: bool,
}

pub async fn run(params: DestroyParams) -> Result<()> {
    match params.provider.as_str() {
        "digitalocean" => run_do(params).await,
        "lightsail" => {
            #[cfg(feature = "lightsail")]
            {
                run_lightsail(params).await
            }
            #[cfg(not(feature = "lightsail"))]
            {
                bail!("Lightsail support not compiled in. Build with --features lightsail")
            }
        }
        "tencent" => run_tencent(params).await,
        _ => {
            let provider = &params.provider;
            bail!("Unknown provider '{provider}'. Use 'digitalocean', 'lightsail', or 'tencent'.")
        }
    }
}

async fn run_do(params: DestroyParams) -> Result<()> {
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

    if !params.yes {
        let confirmed = Confirm::new()
            .with_prompt("Permanently destroy this droplet?")
            .default(false)
            .interact()?;
        if !confirmed {
            println!("Cancelled.");
            return Ok(());
        }
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
        println!("SSH key '{expected_key_name}' not found; skipping.");
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

#[cfg(feature = "lightsail")]
async fn run_lightsail(params: DestroyParams) -> Result<()> {
    use clawmacdo_cloud::CloudProvider;

    let provider = LightsailCliProvider::new(params.do_token.clone());

    println!("Fetching openclaw instances (Lightsail)...");
    let instances = provider.list_instances("openclaw").await?;
    let instance = instances
        .into_iter()
        .find(|i| i.name == params.name)
        .ok_or_else(|| anyhow::anyhow!("No openclaw instance found with name '{}'", params.name))?;

    println!("
Instance to destroy:");
    println!("  Name:   {}", instance.name);
    println!("  IP:     {}", instance.public_ip.as_deref().unwrap_or("N/A"));

    if !params.yes {
        let confirmed = Confirm::new()
            .with_prompt("Permanently destroy this Lightsail instance?")
            .default(false)
            .interact()?;
        if !confirmed {
            println!("Cancelled.");
            return Ok(());
        }
    }

    println!("
Deleting instance '{}'...", instance.name);
    provider.delete_instance(&instance.name).await?;
    println!("Instance deleted.");

    let hostname_suffix = instance.name.strip_prefix("openclaw-").unwrap_or(&instance.name);
    let expected_key_name = format!("clawmacdo-{}", hostname_suffix);
    println!("Attempting to delete SSH key '{}' if present...", expected_key_name);
    let _ = provider.delete_ssh_key(&expected_key_name).await;

    let local_key = config::keys_dir()?.join(format!("clawmacdo_{}", hostname_suffix));
    if local_key.exists() {
        std::fs::remove_file(&local_key)?;
        println!("Removed local key: {}", local_key.display());
    }

    println!("
Destroy complete for '{}'", instance.name);
    Ok(())
}

async fn run_tencent(params: DestroyParams) -> Result<()> {
    let client = TencentClient::new(
        &params.tencent_secret_id,
        &params.tencent_secret_key,
        config::DEFAULT_TENCENT_REGION,
    )?;

    println!("Fetching openclaw instances (Tencent Cloud)...");
    let instances = client.list_openclaw_instances().await?;
    let instance = instances
        .into_iter()
        .find(|i| i.name == params.name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No openclaw instance found with name '{}' on Tencent Cloud",
                params.name
            )
        })?;

    let ip = instance.public_ip.as_deref().unwrap_or("N/A");
    println!();
    println!("Instance to destroy:");
    println!("  Name:   {}", instance.name);
    println!("  ID:     {}", instance.id);
    println!("  IP:     {ip}");
    println!("  Status: {}", instance.status);

    if !params.yes {
        let confirmed = Confirm::new()
            .with_prompt("Permanently destroy this instance?")
            .default(false)
            .interact()?;
        if !confirmed {
            println!("Cancelled.");
            return Ok(());
        }
    }

    println!(
        "\nTerminating instance '{}' (ID {})...",
        instance.name, instance.id
    );
    client.terminate_instance(&instance.id).await?;
    println!("Instance terminated.");

    // Clean up SSH key pair
    let hostname_suffix = instance
        .name
        .strip_prefix("openclaw-")
        .unwrap_or(&instance.name);
    let expected_key_name = format!("clawmacdo-{hostname_suffix}");
    let keys = client.list_key_pairs().await?;
    if let Some((key_id, _)) = keys
        .into_iter()
        .find(|(_, name)| name == &expected_key_name)
    {
        println!("Deleting SSH key pair '{expected_key_name}' (ID {key_id})...");
        client.delete_key_pair(&key_id).await?;
        println!("SSH key pair deleted.");
    } else {
        println!("SSH key pair '{expected_key_name}' not found; skipping.");
    }

    // Clean up local key
    let local_key = config::keys_dir()?.join(format!("clawmacdo_{hostname_suffix}"));
    if local_key.exists() {
        std::fs::remove_file(&local_key)?;
        println!("Removed local key: {}", local_key.display());
    }

    println!("\nDestroy complete for '{}' ({ip}).", instance.name);
    Ok(())
}
