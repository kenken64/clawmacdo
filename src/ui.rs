use crate::config::{self, DeployRecord, OPENCLAW_GATEWAY_PORT};
use console::style;
use dialoguer::{Input, Select};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;

/// Create a spinner with a message.
pub fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg}")
            .expect("valid template"),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Prompt the user to select a region.
pub fn prompt_region() -> Result<String, anyhow::Error> {
    let regions = vec![
        ("nyc1", "New York 1"),
        ("nyc3", "New York 3"),
        ("sfo3", "San Francisco 3"),
        ("ams3", "Amsterdam 3"),
        ("sgp1", "Singapore 1"),
        ("lon1", "London 1"),
        ("fra1", "Frankfurt 1"),
        ("tor1", "Toronto 1"),
        ("blr1", "Bangalore 1"),
        ("syd1", "Sydney 1"),
    ];

    let labels: Vec<String> = regions
        .iter()
        .map(|(slug, name)| format!("{slug} ({name})"))
        .collect();

    let default = regions
        .iter()
        .position(|(s, _)| *s == config::DEFAULT_REGION)
        .unwrap_or(0);

    let selection = Select::new()
        .with_prompt("Select region")
        .items(&labels)
        .default(default)
        .interact()?;

    Ok(regions[selection].0.to_string())
}

/// Prompt the user to select a droplet size.
pub fn prompt_size() -> Result<String, anyhow::Error> {
    let sizes = vec![
        ("s-1vcpu-1gb", "1 vCPU, 1 GB RAM — $6/mo"),
        ("s-1vcpu-2gb", "1 vCPU, 2 GB RAM — $12/mo"),
        ("s-2vcpu-4gb", "2 vCPUs, 4 GB RAM — $24/mo"),
        ("s-4vcpu-8gb", "4 vCPUs, 8 GB RAM — $48/mo"),
        ("s-8vcpu-16gb", "8 vCPUs, 16 GB RAM — $96/mo"),
    ];

    let labels: Vec<String> = sizes
        .iter()
        .map(|(slug, desc)| format!("{slug}  {desc}"))
        .collect();

    let default = sizes
        .iter()
        .position(|(s, _)| *s == config::DEFAULT_SIZE)
        .unwrap_or(0);

    let selection = Select::new()
        .with_prompt("Select droplet size")
        .items(&labels)
        .default(default)
        .interact()?;

    Ok(sizes[selection].0.to_string())
}

/// Prompt for a hostname.
pub fn prompt_hostname(deploy_id: &str) -> Result<String, anyhow::Error> {
    let default = format!("openclaw-{}", &deploy_id[..8.min(deploy_id.len())]);
    let hostname: String = Input::new()
        .with_prompt("Hostname")
        .default(default)
        .interact_text()?;
    Ok(hostname)
}

/// Prompt the user to select a backup archive, or None.
pub fn prompt_backup() -> Result<Option<PathBuf>, anyhow::Error> {
    let backups_dir = config::backups_dir()?;
    let mut entries: Vec<PathBuf> = Vec::new();

    if backups_dir.exists() {
        for entry in std::fs::read_dir(&backups_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("gz") {
                entries.push(path);
            }
        }
    }

    entries.sort();
    entries.reverse(); // newest first

    if entries.is_empty() {
        println!(
            "{}",
            style("No backup archives found. Deploying without config restore.").yellow()
        );
        return Ok(None);
    }

    let mut labels: Vec<String> = entries
        .iter()
        .map(|p| {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let size = std::fs::metadata(p)
                .map(|m| format_bytes(m.len()))
                .unwrap_or_else(|_| "?".into());
            format!("{name}  ({size})")
        })
        .collect();
    labels.push("(No backup)".into());

    let selection = Select::new()
        .with_prompt("Select backup to restore")
        .items(&labels)
        .default(0)
        .interact()?;

    if selection == entries.len() {
        Ok(None)
    } else {
        Ok(Some(entries[selection].clone()))
    }
}

/// Print the deploy summary.
pub fn print_summary(record: &DeployRecord) {
    let divider = "=".repeat(60);
    let ip = &record.ip_address;
    let key = &record.ssh_key_path;
    let port = OPENCLAW_GATEWAY_PORT;

    println!("\n{divider}");
    println!("  OpenClaw Deployment Complete");
    println!("{divider}");
    println!("  Droplet ID:        {}", record.droplet_id);
    println!("  Hostname:          {}", record.hostname);
    println!("  IP Address:        {ip}");
    println!("  Region:            {}", record.region);
    println!("  Size:              {}", record.size);
    println!();
    println!("  SSH Access:");
    println!("    ssh -i {key} root@{ip}");
    println!();
    println!("  OpenClaw Gateway:  http://{ip}:{port}");
    println!("  SSH Private Key:   {key}");
    println!(
        "  Backup Restored:   {}",
        record.backup_restored.as_deref().unwrap_or("None")
    );
    println!(
        "  Deploy Record:     ~/.clawmacdo/deploys/{}.json",
        record.id
    );
    println!();
    println!("  Pre-installed Tools:");
    println!("    - OpenClaw gateway (port {port})");
    println!("    - Claude Code CLI  (claude --version)");
    println!("    - OpenAI Codex CLI (codex --version)");
    println!();
    println!("  API Keys:         /root/.openclaw/.env on server");
    println!("                     (ANTHROPIC_API_KEY + OPENAI_API_KEY + GEMINI_API_KEY)");
    println!("  Messaging Config: (WHATSAPP_PHONE_NUMBER + TELEGRAM_BOT_TOKEN)");
    println!("{divider}");
    println!("  Next steps:");
    println!("    1. ssh -i {key} root@{ip}");
    println!("    2. curl http://{ip}:{port}/health");
    println!("    3. ssh -i {key} root@{ip} journalctl -u openclaw-gateway -f");
    println!("{divider}\n");
}

/// Print the migration summary showing source → target info.
pub fn print_migrate_summary(source_ip: &str, record: &DeployRecord) {
    let divider = "=".repeat(60);
    println!("\n{divider}");
    println!("  OpenClaw Migration Complete");
    println!("{divider}");
    println!("  Source:  {source_ip}");
    println!("  Target:  {} ({})", record.ip_address, record.hostname);
    println!("{divider}");
    print_summary(record);
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
