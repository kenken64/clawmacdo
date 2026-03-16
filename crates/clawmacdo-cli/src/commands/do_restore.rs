use anyhow::{bail, Context, Result};
use chrono::Utc;
use clawmacdo_cloud::digitalocean::DoClient;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_db as db;
use clawmacdo_ssh as ssh;

pub struct DoRestoreParams {
    pub do_token: String,
    pub snapshot_name: String,
    pub region: Option<String>,
    pub size: Option<String>,
}

pub async fn run(params: DoRestoreParams) -> Result<()> {
    config::ensure_dirs()?;

    let deploy_id = uuid::Uuid::new_v4().to_string();
    let hostname = format!("openclaw-{}", &deploy_id[..8]);

    println!("\n[Step 1/5] Resolving parameters...");
    let region = params
        .region
        .unwrap_or_else(|| config::DEFAULT_REGION.to_string());
    let size = params
        .size
        .unwrap_or_else(|| config::DEFAULT_SIZE.to_string());

    println!("  Hostname:  {hostname}");
    println!("  Region:    {region}");
    println!("  Size:      {size}");
    println!("  Snapshot:  {}", params.snapshot_name);

    // Step 2: Generate SSH key pair
    println!("\n[Step 2/5] Generating SSH key pair...");
    let keypair = ssh::generate_keypair(&deploy_id)?;
    println!("  Key saved: {}", keypair.private_key_path.display());

    // Step 3: Upload SSH key and look up snapshot
    println!("\n[Step 3/5] Uploading SSH key and looking up snapshot...");
    let do_client = DoClient::new(&params.do_token)?;

    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    let key_info = do_client
        .upload_ssh_key(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key to DigitalOcean")?;
    println!(
        "  Key ID: {}, Fingerprint: {}",
        key_info.id, key_info.fingerprint
    );

    let snapshots = do_client
        .list_snapshots()
        .await
        .context("Failed to list snapshots")?;

    let snapshot = snapshots
        .iter()
        .find(|s| s.name == params.snapshot_name)
        .ok_or_else(|| {
            let available: Vec<&str> = snapshots.iter().map(|s| s.name.as_str()).collect();
            anyhow::anyhow!(
                "Snapshot '{}' not found. Available snapshots: {:?}",
                params.snapshot_name,
                available
            )
        })?;

    let snapshot_id: u64 = snapshot.id.parse().context(format!(
        "Snapshot ID '{}' is not a valid number",
        snapshot.id
    ))?;
    println!("  Snapshot found: ID {snapshot_id} ({})", snapshot.name);

    if !snapshot.regions.contains(&region) {
        bail!(
            "Snapshot '{}' is not available in region '{region}'. Available regions: {:?}",
            params.snapshot_name,
            snapshot.regions
        );
    }

    // Step 4: Create droplet from snapshot
    println!("\n[Step 4/5] Creating droplet from snapshot...");
    let droplet = do_client
        .create_droplet_from_snapshot(
            &hostname,
            &region,
            &size,
            snapshot_id,
            key_info.id,
            false,
            "",
        )
        .await
        .context("Failed to create droplet from snapshot")?;
    let droplet_id = droplet.id;
    println!("  Droplet created: ID {droplet_id}");

    // Step 5: Wait for droplet to become active
    println!("\n[Step 5/5] Waiting for droplet to become active...");
    let active_droplet = do_client
        .wait_for_active(droplet_id, std::time::Duration::from_secs(300))
        .await
        .context("Droplet did not become active in time")?;

    let ip = active_droplet
        .public_ip()
        .unwrap_or_else(|| "unknown".into());
    println!("  Droplet active: {ip}");

    // Save to SQLite deployments database (for web UI Deployments tab)
    let conn = db::init_db().context("Failed to open deployments database")?;
    db::insert_deployment(
        &conn,
        &deploy_id,
        "snapshot-restore",
        "",
        "digitalocean",
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
        provider: Some(CloudProviderType::DigitalOcean),
        droplet_id,
        instance_id: None,
        hostname: hostname.clone(),
        ip_address: ip.clone(),
        region: region.clone(),
        size: size.clone(),
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: key_info.fingerprint,
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
    println!("  Size:        {size}");
    println!("  Snapshot:    {}", params.snapshot_name);
    println!("  SSH Key:     {}", keypair.private_key_path.display());
    println!("  Record:      {}", record_path.display());
    println!(
        "\n  SSH access:  ssh -i {} root@{ip}",
        keypair.private_key_path.display()
    );

    Ok(())
}
