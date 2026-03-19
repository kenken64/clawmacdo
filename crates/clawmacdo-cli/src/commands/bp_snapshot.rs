use anyhow::{Context, Result};
use clawmacdo_cloud::byteplus::BytePlusClient;
use clawmacdo_ui::spinner;

pub struct BpSnapshotParams {
    pub access_key: String,
    pub secret_key: String,
    pub instance_id: String,
    pub snapshot_name: String,
    pub region: String,
}

pub async fn run(params: BpSnapshotParams) -> Result<()> {
    let bp_client = BytePlusClient::new(&params.access_key, &params.secret_key, &params.region)?;

    // Step 1/4: Verify instance exists
    println!("\n[Step 1/4] Verifying instance...");
    let instance = bp_client
        .describe_instance(&params.instance_id)
        .await
        .context("Failed to get instance")?;
    println!("  Instance: {} (ID {})", instance.name, instance.id);
    println!("  Status:   {}", instance.status);

    // Step 2/4: Find system disk
    println!("\n[Step 2/4] Finding system disk...");
    let volume_id = bp_client
        .describe_system_volume(&params.instance_id)
        .await
        .context("Failed to find system volume")?;
    println!("  System volume: {volume_id}");

    // Step 3/4: Create snapshot
    println!(
        "\n[Step 3/4] Creating snapshot '{}'...",
        params.snapshot_name
    );
    let sp = spinner(&format!("Creating snapshot '{}'...", params.snapshot_name));
    let snapshot_id = bp_client
        .create_ebs_snapshot(&volume_id, &params.snapshot_name)
        .await
        .context("Failed to create snapshot")?;
    println!("  Snapshot ID: {snapshot_id}");

    bp_client
        .wait_for_snapshot(&snapshot_id, std::time::Duration::from_secs(600))
        .await
        .context("Snapshot did not become available in time (10 min timeout)")?;
    sp.finish_with_message("Snapshot available.");

    // Step 4/4: Summary
    println!("\n--- Snapshot Complete ---");
    println!("  Instance:      {} (ID {})", instance.name, instance.id);
    println!("  Volume:        {volume_id}");
    println!("  Snapshot ID:   {snapshot_id}");
    println!("  Snapshot Name: {}", params.snapshot_name);

    Ok(())
}
