use anyhow::{Context, Result};
use chrono::Utc;
use clawmacdo_cloud::byteplus::BytePlusClient;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_db as db;
use clawmacdo_ssh as ssh;
use clawmacdo_ui::spinner;

pub struct BpRestoreParams {
    pub access_key: String,
    pub secret_key: String,
    pub snapshot_name: String,
    pub region: String,
    pub size: Option<String>,
    pub spot: bool,
}

pub async fn run(params: BpRestoreParams) -> Result<()> {
    config::ensure_dirs()?;

    let deploy_id = uuid::Uuid::new_v4().to_string();
    let hostname = format!("openclaw-{}", &deploy_id[..8]);
    let region = params.region;
    let size = params
        .size
        .unwrap_or_else(|| config::DEFAULT_BYTEPLUS_SIZE.to_string());

    // Step 1/7: Resolve parameters
    println!("\n[Step 1/7] Resolving parameters...");
    println!("  Hostname:  {hostname}");
    println!("  Region:    {region}");
    println!("  Size:      {size}");
    println!("  Snapshot:  {}", params.snapshot_name);
    if params.spot {
        println!("  Spot:      enabled (SpotAsPriceGo)");
    }

    let bp_client = BytePlusClient::new(&params.access_key, &params.secret_key, &region)?;

    // Step 2/7: Find snapshot by name
    println!("\n[Step 2/7] Looking up snapshot...");
    let snapshots = bp_client
        .describe_snapshots(Some(&params.snapshot_name))
        .await
        .context("Failed to list snapshots")?;

    let snapshot = snapshots.first().ok_or_else(|| {
        let available: Vec<&str> = snapshots
            .iter()
            .filter_map(|s| s["SnapshotName"].as_str())
            .collect();
        anyhow::anyhow!(
            "Snapshot '{}' not found. Available snapshots: {:?}",
            params.snapshot_name,
            available
        )
    })?;

    let snapshot_id = snapshot["SnapshotId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing SnapshotId in snapshot"))?;
    println!("  Snapshot found: {snapshot_id}");

    // Step 3/7: Create custom image from snapshot
    let image_name = format!("openclaw-img-{}", &deploy_id[..8]);
    println!("\n[Step 3/7] Creating image from snapshot...");
    let sp = spinner("Creating image from snapshot...");
    let image_id = bp_client
        .create_image(snapshot_id, &image_name)
        .await
        .context("Failed to create image from snapshot")?;
    println!("  Image ID: {image_id}");

    bp_client
        .wait_for_image(&image_id, std::time::Duration::from_secs(600))
        .await
        .context("Image did not become available in time (10 min timeout)")?;
    sp.finish_with_message("Image available.");

    // Step 4/7: Generate SSH key pair
    println!("\n[Step 4/7] Generating SSH key pair...");
    let keypair = ssh::generate_keypair(&deploy_id)?;
    println!("  Key saved: {}", keypair.private_key_path.display());

    // Step 5/7: Upload SSH key to BytePlus
    println!("\n[Step 5/7] Uploading SSH key...");
    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    let key_info = bp_client
        .import_key_pair(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key")?;
    println!("  Key ID: {}", key_info.id);

    // Step 6/7: Create instance from custom image
    println!("\n[Step 6/7] Creating instance from image...");
    let instance_id = bp_client
        .create_instance_from_image(&hostname, &image_id, &size, &key_name, "", params.spot)
        .await
        .context("Failed to create instance from image")?;
    println!("  Instance created: {instance_id}");

    // Step 7/7: Wait for instance to become RUNNING
    println!("\n[Step 7/7] Waiting for instance to become active...");
    let sp = spinner("Waiting for instance...");
    let instance = bp_client
        .wait_for_running(&instance_id, std::time::Duration::from_secs(300))
        .await
        .context("Instance did not become RUNNING in time")?;

    let ip = instance.public_ip.unwrap_or_else(|| "unknown".into());
    sp.finish_with_message(format!("Instance active at {ip}"));

    // Save to SQLite
    let conn = db::init_db().context("Failed to open deployments database")?;
    db::insert_deployment(
        &conn,
        &deploy_id,
        "snapshot-restore",
        "",
        "byteplus",
        &region,
        &size,
        &hostname,
    )
    .context("Failed to insert deployment record")?;
    db::update_deployment_status(&conn, &deploy_id, "completed", Some(&ip), Some(&hostname))
        .context("Failed to update deployment status")?;

    // Save JSON deploy record
    let record = DeployRecord {
        id: deploy_id.clone(),
        provider: Some(CloudProviderType::BytePlus),
        droplet_id: 0,
        instance_id: Some(instance_id),
        hostname: hostname.clone(),
        ip_address: ip.clone(),
        region: region.clone(),
        size: size.clone(),
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: key_info.fingerprint.unwrap_or_default(),
        ssh_key_id: Some(key_info.id),
        resource_group: None,
        backup_restored: None,
        created_at: Utc::now(),
    };
    let record_path = record.save()?;

    println!("\n--- Restore Complete ---");
    println!("  Deploy ID:   {deploy_id}");
    println!("  Hostname:    {hostname}");
    println!("  IP Address:  {ip}");
    println!("  Region:      {region}");
    println!("  Size:        {size}");
    println!("  Snapshot:    {}", params.snapshot_name);
    println!("  Image:       {image_id}");
    println!("  SSH Key:     {}", keypair.private_key_path.display());
    println!("  Record:      {}", record_path.display());
    println!(
        "\n  SSH access:  ssh -i {} root@{ip}",
        keypair.private_key_path.display()
    );

    Ok(())
}
