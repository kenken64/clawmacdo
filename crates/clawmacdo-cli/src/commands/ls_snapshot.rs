use anyhow::{Context, Result};
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_cloud::CloudProvider;
use clawmacdo_ui::spinner;

pub struct LsSnapshotParams {
    pub instance_name: String,
    pub snapshot_name: String,
    pub region: String,
}

pub async fn run(params: LsSnapshotParams) -> Result<()> {
    let provider = LightsailCliProvider::new(params.region.clone());

    // Step 1/3: Verify instance exists
    println!("\n[Step 1/3] Verifying instance...");
    let instance = provider
        .wait_for_active(&params.instance_name, 5)
        .await
        .context("Instance not found or not running")?;
    println!("  Instance: {} ({})", instance.name, instance.status);
    if let Some(ip) = &instance.public_ip {
        println!("  IP:       {ip}");
    }

    // Step 2/3: Create snapshot
    println!(
        "\n[Step 2/3] Creating snapshot '{}'...",
        params.snapshot_name
    );
    let sp = spinner(&format!("Creating snapshot '{}'...", params.snapshot_name));
    provider
        .create_instance_snapshot(&params.instance_name, &params.snapshot_name)
        .context("Failed to create snapshot")?;

    provider
        .wait_for_snapshot(&params.snapshot_name, std::time::Duration::from_secs(600))
        .await
        .context("Snapshot did not become available in time (10 min timeout)")?;
    sp.finish_with_message("Snapshot available.");

    // Step 3/3: Confirm
    println!("\n[Step 3/3] Confirming snapshot...");
    let snap = provider
        .get_snapshot(&params.snapshot_name)
        .context("Failed to get snapshot details")?;
    println!("  Size: {} GB", snap.size_in_gb.unwrap_or(0));

    println!("\n--- Snapshot Complete ---");
    println!("  Instance:      {}", params.instance_name);
    println!("  Snapshot Name: {}", params.snapshot_name);
    println!("  Region:        {}", params.region);

    Ok(())
}
