use anyhow::{bail, Result};
use clawmacdo_db as db;
use console::Style;

pub struct TrackParams {
    pub query: String,
    pub follow: bool,
    pub json: bool,
}

pub async fn run(params: TrackParams) -> Result<()> {
    let conn = db::init_db()?;

    let deployment = match db::find_deployment_by_query(&conn, &params.query)? {
        Some(d) => d,
        None => bail!("No deployment found matching '{}'", params.query),
    };

    if params.follow {
        run_follow(&conn, &deployment, params.json).await
    } else {
        render_once(&conn, &deployment, params.json)
    }
}

fn render_once(
    conn: &rusqlite::Connection,
    deployment: &db::DeploymentRow,
    json: bool,
) -> Result<()> {
    let steps = db::get_deploy_steps(conn, &deployment.id)?;

    if json {
        render_json(deployment, &steps);
    } else {
        render_human(deployment, &steps);
    }
    Ok(())
}

async fn run_follow(
    conn: &rusqlite::Connection,
    deployment: &db::DeploymentRow,
    json: bool,
) -> Result<()> {
    let deploy_id = &deployment.id;
    let term = console::Term::stdout();
    let mut last_step_count = 0usize;
    let mut last_statuses: Vec<String> = Vec::new();

    loop {
        // Re-fetch deployment status
        let current = db::get_deployment_by_id(conn, deploy_id)?.unwrap_or_else(|| unreachable!());
        let steps = db::get_deploy_steps(conn, deploy_id)?;

        if json {
            // Only emit new/changed steps
            let current_statuses: Vec<String> = steps
                .iter()
                .map(|s| format!("{}:{}", s.step_number, s.status))
                .collect();
            if current_statuses != last_statuses {
                if last_step_count == 0 {
                    // First render — emit header + all steps
                    render_json(&current, &steps);
                } else {
                    // Emit only changed steps
                    for step in &steps {
                        let key = format!("{}:{}", step.step_number, step.status);
                        if !last_statuses.contains(&key) {
                            let line = serde_json::json!({
                                "event": "step",
                                "step": step.step_number,
                                "total": step.total_steps,
                                "label": step.label,
                                "status": step.status,
                                "started_at": step.started_at,
                                "completed_at": step.completed_at,
                                "error_msg": step.error_msg,
                            });
                            println!("{line}");
                        }
                    }
                }
                last_statuses = current_statuses;
            }
        } else {
            let _ = term.clear_screen();
            render_human(&current, &steps);
        }

        last_step_count = steps.len();

        if current.status != "running" {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    Ok(())
}

fn render_human(deployment: &db::DeploymentRow, steps: &[db::DeployStepRow]) {
    let bold = Style::new().bold();
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let red = Style::new().red();
    let dim = Style::new().dim();

    println!("{}", bold.apply_to("Deploy Tracking"));
    println!("  ID:       {}", deployment.id);
    println!(
        "  Provider: {}",
        deployment.provider.as_deref().unwrap_or("unknown")
    );
    println!(
        "  IP:       {}",
        deployment.ip_address.as_deref().unwrap_or("pending")
    );
    println!("  Status:   {}", format_status(&deployment.status));
    println!("  Created:  {}", deployment.created_at);
    println!();

    if steps.is_empty() {
        println!("  No steps recorded yet.");
        return;
    }

    for step in steps {
        let indicator = match step.status.as_str() {
            "completed" => green.apply_to("✓").to_string(),
            "running" => yellow.apply_to("▸").to_string(),
            "skipped" => dim.apply_to("—").to_string(),
            "failed" => red.apply_to("✗").to_string(),
            _ => " ".to_string(),
        };
        println!(
            "  {} [{:>2}/{}] {}",
            indicator, step.step_number, step.total_steps, step.label
        );
        if let Some(err) = &step.error_msg {
            println!("      {}", red.apply_to(err));
        }
    }
}

fn render_json(deployment: &db::DeploymentRow, steps: &[db::DeployStepRow]) {
    let header = serde_json::json!({
        "event": "tracking",
        "deploy_id": deployment.id,
        "provider": deployment.provider,
        "status": deployment.status,
        "ip": deployment.ip_address,
        "hostname": deployment.hostname,
        "created_at": deployment.created_at,
    });
    println!("{header}");

    for step in steps {
        let line = serde_json::json!({
            "event": "step",
            "step": step.step_number,
            "total": step.total_steps,
            "label": step.label,
            "status": step.status,
            "started_at": step.started_at,
            "completed_at": step.completed_at,
            "error_msg": step.error_msg,
        });
        println!("{line}");
    }
}

fn format_status(status: &str) -> String {
    match status {
        "running" => console::Style::new().yellow().apply_to(status).to_string(),
        "completed" => console::Style::new().green().apply_to(status).to_string(),
        "failed" => console::Style::new().red().apply_to(status).to_string(),
        _ => status.to_string(),
    }
}
