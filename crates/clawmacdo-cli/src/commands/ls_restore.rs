use anyhow::{Context, Result};
use chrono::Utc;
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_cloud::CloudProvider;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_db as db;
use clawmacdo_ssh as ssh;

pub struct LsRestoreParams {
    pub snapshot_name: String,
    pub region: String,
    pub size: Option<String>,
}

pub async fn run(params: LsRestoreParams) -> Result<()> {
    config::ensure_dirs()?;

    let deploy_id = uuid::Uuid::new_v4().to_string();
    let hostname = format!("openclaw-{}", &deploy_id[..8]);
    let region = params.region;
    let size = params
        .size
        .unwrap_or_else(|| config::DEFAULT_SIZE.to_string());

    let provider = LightsailCliProvider::new(region.clone());
    let bundle_id = provider.get_bundle_id(&size);

    // Step 1/5: Resolve parameters
    println!("\n[Step 1/5] Resolving parameters...");
    println!("  Hostname:  {hostname}");
    println!("  Region:    {region}");
    println!("  Size:      {size} (bundle: {bundle_id})");
    println!("  Snapshot:  {}", params.snapshot_name);

    // Step 2/5: Verify snapshot exists
    println!("\n[Step 2/5] Looking up snapshot...");
    let snap = provider
        .get_snapshot(&params.snapshot_name)
        .context("Snapshot not found")?;
    println!(
        "  Snapshot found: {} ({} GB, from: {})",
        snap.name.as_deref().unwrap_or("?"),
        snap.size_in_gb.unwrap_or(0),
        snap.from_instance_name.as_deref().unwrap_or("?")
    );

    // Step 3/5: Generate SSH key pair and upload
    println!("\n[Step 3/5] Generating and uploading SSH key...");
    let keypair = ssh::generate_keypair(&deploy_id)?;
    println!("  Key saved: {}", keypair.private_key_path.display());

    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    let key_info = provider
        .upload_ssh_key(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key")?;
    println!("  Key uploaded: {}", key_info.id);

    // Step 4/5: Create instance from snapshot
    println!("\n[Step 4/5] Creating instance from snapshot...");
    provider
        .create_instance_from_snapshot(&hostname, &params.snapshot_name, &bundle_id, &key_name)
        .context("Failed to create instance from snapshot")?;
    println!("  Instance creation started: {hostname}");

    // Step 5/5: Wait for instance to become active
    println!("\n[Step 5/5] Waiting for instance to become active...");
    let instance = provider
        .wait_for_active(&hostname, 300)
        .await
        .context("Instance did not become active in time")?;

    let ip = instance.public_ip.unwrap_or_else(|| "unknown".into());
    println!("  Instance active: {ip}");

    // Save to SQLite
    let conn = db::init_db().context("Failed to open deployments database")?;
    db::insert_deployment(
        &conn,
        &deploy_id,
        "snapshot-restore",
        "",
        "lightsail",
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
        provider: Some(CloudProviderType::Lightsail),
        droplet_id: 0,
        instance_id: None,
        hostname: hostname.clone(),
        ip_address: ip.clone(),
        region: region.clone(),
        size: size.clone(),
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: key_info.fingerprint.unwrap_or_default(),
        ssh_key_id: None,
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
    println!("  Size:        {size} ({bundle_id})");
    println!("  Snapshot:    {}", params.snapshot_name);
    println!("  SSH Key:     {}", keypair.private_key_path.display());
    println!("  Record:      {}", record_path.display());
    println!(
        "\n  SSH access:  ssh -i {} ubuntu@{ip}",
        keypair.private_key_path.display()
    );

    Ok(())
}
