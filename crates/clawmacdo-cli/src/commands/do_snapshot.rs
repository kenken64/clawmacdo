use anyhow::{bail, Context, Result};
use clawmacdo_cloud::digitalocean::DoClient;
use clawmacdo_db as db;
use clawmacdo_ui::{progress, spinner};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct DoSnapshotParams {
    pub do_token: String,
    pub droplet_id: u64,
    pub snapshot_name: String,
    pub power_off: bool,
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
    pub db: Option<Arc<Mutex<rusqlite::Connection>>>,
    pub op_id: Option<String>,
}

pub async fn run(params: DoSnapshotParams) -> Result<()> {
    let total_steps: i32 = if params.power_off { 5 } else { 3 };
    let tx = &params.progress_tx;
    let db = &params.db;
    let op_id = params.op_id.as_deref().unwrap_or("");

    // Step 1: Verify droplet exists
    progress::emit(
        tx,
        &format!("\n[Step 1/{total_steps}] Verifying droplet..."),
    );
    db::record_step_start(db, op_id, 1, total_steps, "Verifying droplet");
    let do_client = DoClient::new(&params.do_token)?;
    let droplet = do_client
        .get_droplet(params.droplet_id)
        .await
        .context("Failed to get droplet")?;
    progress::emit(
        tx,
        &format!(
            "  Droplet: {} (ID {}) — {}",
            droplet.name, droplet.id, droplet.status
        ),
    );
    db::record_step_complete(db, op_id, 1);

    let mut step: i32 = 2;

    // Optional: Power off the droplet
    if params.power_off {
        progress::emit(
            tx,
            &format!("\n[Step {step}/{total_steps}] Shutting down droplet..."),
        );
        db::record_step_start(db, op_id, step, total_steps, "Shutting down droplet");
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
            progress::emit(tx, "  Droplet shut down.");
        } else if droplet.status == "off" {
            progress::emit(tx, "  Droplet is already off, skipping shutdown.");
        } else {
            db::record_step_failed(
                db,
                op_id,
                step,
                &format!("Droplet in '{}' state", droplet.status),
            );
            bail!(
                "Droplet is in '{}' state; cannot shut down. Expected 'active' or 'off'.",
                droplet.status
            );
        }
        db::record_step_complete(db, op_id, step);
        step += 1;
    }

    // Create snapshot
    progress::emit(
        tx,
        &format!(
            "\n[Step {step}/{total_steps}] Creating snapshot '{}'...",
            params.snapshot_name
        ),
    );
    db::record_step_start(db, op_id, step, total_steps, "Creating snapshot");
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
    progress::emit(tx, "  Snapshot created.");
    db::record_step_complete(db, op_id, step);
    step += 1;

    // Optional: Power on the droplet
    if params.power_off {
        progress::emit(
            tx,
            &format!("\n[Step {step}/{total_steps}] Powering on droplet..."),
        );
        db::record_step_start(db, op_id, step, total_steps, "Powering on droplet");
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
        progress::emit(tx, "  Droplet powered on.");
        db::record_step_complete(db, op_id, step);
        step += 1;
    }

    // Confirm snapshot
    progress::emit(
        tx,
        &format!("\n[Step {step}/{total_steps}] Confirming snapshot..."),
    );
    db::record_step_start(db, op_id, step, total_steps, "Confirming snapshot");
    let snapshots = do_client
        .get_droplet_snapshots(droplet.id)
        .await
        .context("Failed to list droplet snapshots")?;

    if let Some(snap) = snapshots.iter().find(|s| s.name == params.snapshot_name) {
        progress::emit(
            tx,
            &format!("  Snapshot confirmed: ID {} ({})", snap.id, snap.name),
        );
    } else {
        progress::emit(
            tx,
            &format!(
                "  Warning: snapshot '{}' not found yet (may still be propagating).",
                params.snapshot_name
            ),
        );
    }
    db::record_step_complete(db, op_id, step);

    progress::emit(tx, "\n--- Snapshot Complete ---");
    progress::emit(
        tx,
        &format!("  Droplet:       {} (ID {})", droplet.name, droplet.id),
    );
    progress::emit(tx, &format!("  Snapshot Name: {}", params.snapshot_name));

    Ok(())
}
