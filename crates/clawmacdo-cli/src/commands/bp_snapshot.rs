use anyhow::{Context, Result};
use clawmacdo_cloud::byteplus::BytePlusClient;
use clawmacdo_db as db;
use clawmacdo_ui::{progress, spinner};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct BpSnapshotParams {
    pub access_key: String,
    pub secret_key: String,
    pub instance_id: String,
    pub snapshot_name: String,
    pub region: String,
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
    pub db: Option<Arc<Mutex<rusqlite::Connection>>>,
    pub op_id: Option<String>,
}

pub async fn run(params: BpSnapshotParams) -> Result<()> {
    let bp_client = BytePlusClient::new(&params.access_key, &params.secret_key, &params.region)?;
    let tx = &params.progress_tx;
    let pdb = &params.db;
    let op_id = params.op_id.as_deref().unwrap_or("");
    let total: i32 = 4;

    // Step 1/4: Verify instance exists
    progress::emit(tx, &format!("\n[Step 1/{total}] Verifying instance..."));
    db::record_step_start(pdb, op_id, 1, total, "Verifying instance");
    let instance = bp_client
        .describe_instance(&params.instance_id)
        .await
        .context("Failed to get instance")?;
    progress::emit(
        tx,
        &format!(
            "  Instance: {} (ID {}) — {}",
            instance.name, instance.id, instance.status
        ),
    );
    db::record_step_complete(pdb, op_id, 1);

    // Step 2/4: Find system disk
    progress::emit(tx, &format!("\n[Step 2/{total}] Finding system disk..."));
    db::record_step_start(pdb, op_id, 2, total, "Finding system disk");
    let volume_id = bp_client
        .describe_system_volume(&params.instance_id)
        .await
        .context("Failed to find system volume")?;
    progress::emit(tx, &format!("  System volume: {volume_id}"));
    db::record_step_complete(pdb, op_id, 2);

    // Step 3/4: Create snapshot
    progress::emit(
        tx,
        &format!(
            "\n[Step 3/{total}] Creating snapshot '{}'...",
            params.snapshot_name
        ),
    );
    db::record_step_start(pdb, op_id, 3, total, "Creating snapshot");
    let sp = spinner(&format!("Creating snapshot '{}'...", params.snapshot_name));
    let snapshot_id = bp_client
        .create_ebs_snapshot(&volume_id, &params.snapshot_name)
        .await
        .context("Failed to create snapshot")?;
    progress::emit(tx, &format!("  Snapshot ID: {snapshot_id}"));

    bp_client
        .wait_for_snapshot(&snapshot_id, std::time::Duration::from_secs(600))
        .await
        .context("Snapshot did not become available in time (10 min timeout)")?;
    sp.finish_with_message("Snapshot available.");
    progress::emit(tx, "  Snapshot available.");
    db::record_step_complete(pdb, op_id, 3);

    // Step 4/4: Summary
    progress::emit(tx, &format!("\n[Step 4/{total}] Confirming snapshot..."));
    db::record_step_start(pdb, op_id, 4, total, "Confirming snapshot");
    progress::emit(tx, "\n--- Snapshot Complete ---");
    progress::emit(
        tx,
        &format!("  Instance:      {} (ID {})", instance.name, instance.id),
    );
    progress::emit(tx, &format!("  Volume:        {volume_id}"));
    progress::emit(tx, &format!("  Snapshot ID:   {snapshot_id}"));
    progress::emit(tx, &format!("  Snapshot Name: {}", params.snapshot_name));
    db::record_step_complete(pdb, op_id, 4);

    Ok(())
}
