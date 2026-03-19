use anyhow::{bail, Context, Result};
use clawmacdo_cloud::digitalocean::DoClient;
use clawmacdo_ui::spinner;

pub struct DoSnapshotParams {
    pub do_token: String,
    pub droplet_id: u64,
    pub snapshot_name: String,
    pub power_off: bool,
}

pub async fn run(params: DoSnapshotParams) -> Result<()> {
    let total_steps = if params.power_off { 5 } else { 3 };

    // Step 1: Verify droplet exists
    println!("\n[Step 1/{total_steps}] Verifying droplet...");
    let do_client = DoClient::new(&params.do_token)?;
    let droplet = do_client
        .get_droplet(params.droplet_id)
        .await
        .context("Failed to get droplet")?;
    println!("  Droplet:   {} (ID {})", droplet.name, droplet.id);
    println!("  Status:    {}", droplet.status);
    println!("  Region:    {}", droplet.region.slug);

    let mut step = 2;

    // Optional: Power off the droplet
    if params.power_off {
        println!("\n[Step {step}/{total_steps}] Shutting down droplet...");
        if droplet.status == "active" {
            let sp = spinner("Shutting down...");
            let action_id = do_client
                .shutdown_droplet(droplet.id)
                .await
                .context("Failed to initiate shutdown")?;
            do_client
                .wait_for_action(action_id, std::time::Duration::from_secs(120))
                .await
                .context("Shutdown did not complete in time")?;
            sp.finish_with_message("Droplet shut down.");
        } else if droplet.status == "off" {
            println!("  Droplet is already off, skipping shutdown.");
        } else {
            bail!(
                "Droplet is in '{}' state; cannot shut down. Expected 'active' or 'off'.",
                droplet.status
            );
        }
        step += 1;
    }

    // Create snapshot
    println!(
        "\n[Step {step}/{total_steps}] Creating snapshot '{}'...",
        params.snapshot_name
    );
    let sp = spinner(&format!("Creating snapshot '{}'...", params.snapshot_name));
    let action_id = do_client
        .create_snapshot(droplet.id, &params.snapshot_name)
        .await
        .context("Failed to initiate snapshot")?;

    do_client
        .wait_for_action(action_id, std::time::Duration::from_secs(600))
        .await
        .context("Snapshot did not complete in time (10 min timeout)")?;
    sp.finish_with_message("Snapshot created.");
    step += 1;

    // Optional: Power on the droplet
    if params.power_off {
        println!("\n[Step {step}/{total_steps}] Powering on droplet...");
        let sp = spinner("Powering on...");
        let action_id = do_client
            .power_on_droplet(droplet.id)
            .await
            .context("Failed to initiate power on")?;
        do_client
            .wait_for_action(action_id, std::time::Duration::from_secs(120))
            .await
            .context("Power on did not complete in time")?;
        sp.finish_with_message("Droplet powered on.");
        step += 1;
    }

    // Confirm snapshot
    println!("\n[Step {step}/{total_steps}] Confirming snapshot...");
    let snapshots = do_client
        .get_droplet_snapshots(droplet.id)
        .await
        .context("Failed to list droplet snapshots")?;

    if let Some(snap) = snapshots.iter().find(|s| s.name == params.snapshot_name) {
        println!("  Snapshot confirmed: ID {} ({})", snap.id, snap.name);
        println!("  Regions: {:?}", snap.regions);
    } else {
        println!(
            "  Warning: snapshot '{}' not found in droplet snapshots list yet (may still be propagating).",
            params.snapshot_name
        );
    }

    println!("\n--- Snapshot Complete ---");
    println!("  Droplet:       {} (ID {})", droplet.name, droplet.id);
    println!("  Snapshot Name: {}", params.snapshot_name);

    Ok(())
}
