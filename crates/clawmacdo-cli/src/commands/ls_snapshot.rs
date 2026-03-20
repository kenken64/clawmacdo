use anyhow::{Context, Result};
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_cloud::CloudProvider;
use clawmacdo_db as db;
use clawmacdo_ui::{progress, spinner};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct LsSnapshotParams {
    pub instance_name: String,
    pub snapshot_name: String,
    pub region: String,
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
    pub db: Option<Arc<Mutex<rusqlite::Connection>>>,
    pub op_id: Option<String>,
}

pub async fn run(params: LsSnapshotParams) -> Result<()> {
    let provider = LightsailCliProvider::new(params.region.clone());
    let tx = &params.progress_tx;
    let pdb = &params.db;
    let op_id = params.op_id.as_deref().unwrap_or("");
    let total: i32 = 3;

    // Step 1/3: Verify instance exists
    progress::emit(tx, &format!("\n[Step 1/{total}] Verifying instance..."));
    db::record_step_start(pdb, op_id, 1, total, "Verifying instance");
    let instance = provider
        .wait_for_active(&params.instance_name, 5)
        .await
        .context("Instance not found or not running")?;
    progress::emit(
        tx,
        &format!("  Instance: {} ({})", instance.name, instance.status),
    );
    if let Some(ip) = &instance.public_ip {
        progress::emit(tx, &format!("  IP:       {ip}"));
    }
    db::record_step_complete(pdb, op_id, 1);

    // Step 2/3: Create snapshot
    progress::emit(
        tx,
        &format!(
            "\n[Step 2/{total}] Creating snapshot '{}'...",
            params.snapshot_name
        ),
    );
    db::record_step_start(pdb, op_id, 2, total, "Creating snapshot");
    let sp = spinner(&format!("Creating snapshot '{}'...", params.snapshot_name));
    provider
        .create_instance_snapshot(&params.instance_name, &params.snapshot_name)
        .context("Failed to create snapshot")?;

    provider
        .wait_for_snapshot(&params.snapshot_name, std::time::Duration::from_secs(600))
        .await
        .context("Snapshot did not become available in time (10 min timeout)")?;
    sp.finish_with_message("Snapshot available.");
    progress::emit(tx, "  Snapshot available.");
    db::record_step_complete(pdb, op_id, 2);

    // Step 3/3: Confirm
    progress::emit(tx, &format!("\n[Step 3/{total}] Confirming snapshot..."));
    db::record_step_start(pdb, op_id, 3, total, "Confirming snapshot");
    let snap = provider
        .get_snapshot(&params.snapshot_name)
        .context("Failed to get snapshot details")?;
    progress::emit(tx, &format!("  Size: {} GB", snap.size_in_gb.unwrap_or(0)));
    db::record_step_complete(pdb, op_id, 3);

    progress::emit(tx, "\n--- Snapshot Complete ---");
    progress::emit(tx, &format!("  Instance:      {}", params.instance_name));
    progress::emit(tx, &format!("  Snapshot Name: {}", params.snapshot_name));
    progress::emit(tx, &format!("  Region:        {}", params.region));

    Ok(())
}
