use anyhow::{bail, Result};
use clawmacdo_cloud::digitalocean::DoClient;
#[cfg(feature = "lightsail")]
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_cloud::tencent::TencentClient;
use clawmacdo_core::config;
use dialoguer::Confirm;

pub struct DestroyParams {
    pub provider: String,
    pub do_token: String,
    pub tencent_secret_id: String,
    pub tencent_secret_key: String,
    pub aws_region: String,
    pub azure_tenant_id: String,
    pub azure_subscription_id: String,
    pub azure_client_id: String,
    pub azure_client_secret: String,
    pub azure_resource_group: String,
    pub byteplus_access_key: String,
    pub byteplus_secret_key: String,
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
        "azure" => {
            #[cfg(feature = "azure")]
            {
                run_azure(params).await
            }
            #[cfg(not(feature = "azure"))]
            {
                bail!("Azure support not compiled in. Build with --features azure")
            }
        }
        "byteplus" | "bp" => {
            #[cfg(feature = "byteplus")]
            {
                run_byteplus(params).await
            }
            #[cfg(not(feature = "byteplus"))]
            {
                bail!("BytePlus support not compiled in. Build with --features byteplus")
            }
        }
        _ => {
            let provider = &params.provider;
            bail!("Unknown provider '{provider}'. Use 'digitalocean', 'lightsail', 'tencent', 'azure', or 'byteplus'.")
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

    clawmacdo_cloud::lightsail_cli::ensure_aws_cli()?;
    let provider = LightsailCliProvider::new(params.aws_region.clone());

    println!("Fetching openclaw instances (Lightsail)...");
    let instances = provider.list_instances("openclaw").await?;
    let instance = instances
        .into_iter()
        .find(|i| i.name == params.name)
        .ok_or_else(|| anyhow::anyhow!("No openclaw instance found with name '{}'", params.name))?;

    println!(
        "
Instance to destroy:"
    );
    println!("  Name:   {}", instance.name);
    println!(
        "  IP:     {}",
        instance.public_ip.as_deref().unwrap_or("N/A")
    );

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

    println!(
        "
Deleting instance '{}'...",
        instance.name
    );
    provider.delete_instance(&instance.name).await?;
    println!("Instance deleted.");

    let hostname_suffix = instance
        .name
        .strip_prefix("openclaw-")
        .unwrap_or(&instance.name);
    let expected_key_name = format!("clawmacdo-{hostname_suffix}");
    println!("Attempting to delete SSH key '{expected_key_name}' if present...");
    let _ = provider.delete_ssh_key(&expected_key_name).await;

    let local_key = config::keys_dir()?.join(format!("clawmacdo_{hostname_suffix}"));
    if local_key.exists() {
        std::fs::remove_file(&local_key)?;
        println!("Removed local key: {}", local_key.display());
    }

    println!(
        "
Destroy complete for '{}'",
        instance.name
    );
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

#[cfg(feature = "azure")]
async fn run_azure(params: DestroyParams) -> Result<()> {
    use clawmacdo_cloud::azure_cli::{self, AzureCliProvider};
    use clawmacdo_cloud::CloudProvider;

    azure_cli::ensure_az_cli()?;
    azure_cli::az_login(
        &params.azure_tenant_id,
        &params.azure_client_id,
        &params.azure_client_secret,
    )?;
    azure_cli::az_set_subscription(&params.azure_subscription_id)?;

    let provider = AzureCliProvider::new(
        String::new(), // region not needed for destroy
        params.azure_resource_group.clone(),
        params.azure_subscription_id.clone(),
    );

    println!("Fetching openclaw instances (Azure)...");
    let instances = provider.list_instances("openclaw").await?;

    if instances.is_empty() {
        println!(
            "No instances found in resource group '{}'.",
            params.azure_resource_group
        );
    } else {
        let instance = instances
            .into_iter()
            .find(|i| i.name == params.name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No openclaw instance found with name '{}' in resource group '{}'",
                    params.name,
                    params.azure_resource_group
                )
            })?;

        println!("\nInstance to destroy:");
        println!("  Name:   {}", instance.name);
        println!(
            "  IP:     {}",
            instance.public_ip.as_deref().unwrap_or("N/A")
        );
        println!("  Status: {}", instance.status);

        if !params.yes {
            let confirmed = Confirm::new()
                .with_prompt("Permanently destroy this Azure VM and its resource group?")
                .default(false)
                .interact()?;
            if !confirmed {
                println!("Cancelled.");
                return Ok(());
            }
        }
    }

    println!(
        "\nDeleting resource group '{}' (this removes all resources)...",
        params.azure_resource_group
    );
    provider.delete_resource_group()?;
    println!("Resource group deletion initiated (--no-wait).");

    // Clean up local key
    let hostname_suffix = params
        .name
        .strip_prefix("openclaw-")
        .unwrap_or(&params.name);
    let local_key = config::keys_dir()?.join(format!("clawmacdo_{hostname_suffix}"));
    if local_key.exists() {
        std::fs::remove_file(&local_key)?;
        println!("Removed local key: {}", local_key.display());
    }

    println!(
        "\nDestroy complete for '{}' (resource group: {}).",
        params.name, params.azure_resource_group
    );
    Ok(())
}

#[cfg(feature = "byteplus")]
async fn run_byteplus(params: DestroyParams) -> Result<()> {
    use clawmacdo_cloud::byteplus::BytePlusClient;

    let client = BytePlusClient::new(
        &params.byteplus_access_key,
        &params.byteplus_secret_key,
        config::DEFAULT_BYTEPLUS_REGION,
    )?;

    println!("Fetching instances (BytePlus ECS)...");
    let instances = if params.name.is_empty() {
        client.list_all_instances().await?
    } else {
        client.list_openclaw_instances().await?
    };

    if instances.is_empty() {
        println!("No instances found.");
        return Ok(());
    }

    if params.name.is_empty() {
        // Destroy all instances
        println!("Found {} instance(s):", instances.len());
        for inst in &instances {
            let ip = inst.public_ip.as_deref().unwrap_or("N/A");
            println!("  {} | {} | {} | {ip}", inst.id, inst.name, inst.status);
        }

        if !params.yes {
            let confirmed = Confirm::new()
                .with_prompt("Permanently destroy ALL these instances?")
                .default(false)
                .interact()?;
            if !confirmed {
                println!("Cancelled.");
                return Ok(());
            }
        }

        for inst in &instances {
            println!("\nTerminating '{}' (ID {})...", inst.name, inst.id);
            // Release EIP before terminating
            if let Ok(Some(alloc_id)) = client.describe_instance_eip(&inst.id).await {
                println!("  Releasing EIP ({alloc_id})...");
                let _ = client.disassociate_eip(&alloc_id).await;
                let _ = client.release_eip(&alloc_id).await;
            } else {
                println!("  No EIP found (may auto-release with instance).");
            }
            client.terminate_instance(&inst.id).await?;
            println!("  Terminated.");
        }

        // Clean up VPC resources (security groups, subnets, VPC)
        println!("\nCleaning up VPC resources (waiting for instance deletion to propagate)...");
        client.cleanup_vpc_resources().await;
        println!("VPC cleanup complete.");
    } else {
        let instance = instances
            .into_iter()
            .find(|i| i.name == params.name || i.id == params.name)
            .ok_or_else(|| {
                anyhow::anyhow!("No openclaw instance found with name/id '{}'", params.name)
            })?;

        let ip = instance.public_ip.as_deref().unwrap_or("N/A");
        println!("\nInstance to destroy:");
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

        println!("\nTerminating '{}' (ID {})...", instance.name, instance.id);

        // Release EIP before terminating
        if let Ok(Some(alloc_id)) = client.describe_instance_eip(&instance.id).await {
            println!("  Releasing EIP ({alloc_id})...");
            let _ = client.disassociate_eip(&alloc_id).await;
            let _ = client.release_eip(&alloc_id).await;
            println!("  EIP released.");
        } else {
            println!("  No EIP found (may auto-release with instance).");
        }

        client.terminate_instance(&instance.id).await?;
        println!("Instance terminated.");

        // Clean up VPC resources (security groups, subnets, VPC)
        // Waits for instance deletion to fully propagate before removing network resources.
        println!("Cleaning up VPC resources (waiting for instance deletion to propagate)...");
        client.cleanup_vpc_resources().await;
        println!("VPC cleanup complete.");

        // Clean up local key
        let hostname_suffix = instance
            .name
            .strip_prefix("openclaw-")
            .unwrap_or(&instance.name);
        let local_key = config::keys_dir()?.join(format!("clawmacdo_{hostname_suffix}"));
        if local_key.exists() {
            std::fs::remove_file(&local_key)?;
            println!("Removed local key: {}", local_key.display());
        }

        println!("\nDestroy complete for '{}' ({ip}).", instance.name);
    }

    Ok(())
}
