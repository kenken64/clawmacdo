use crate::config;
use anyhow::Result;
use chrono::{DateTime, Utc};

/// List all backup archives in ~/.clawmacdo/backups/ with sizes and dates.
pub fn run() -> Result<()> {
    let backups_dir = config::backups_dir()?;

    if !backups_dir.exists() {
        println!("No backups directory found at {}", backups_dir.display());
        return Ok(());
    }

    let mut entries: Vec<(String, u64, DateTime<Utc>)> = Vec::new();

    for entry in std::fs::read_dir(&backups_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("gz") {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let meta = std::fs::metadata(&path)?;
            let size = meta.len();
            let modified: DateTime<Utc> = meta.modified()?.into();
            entries.push((name, size, modified));
        }
    }

    if entries.is_empty() {
        println!("No backup archives found in {}", backups_dir.display());
        return Ok(());
    }

    entries.sort_by(|a, b| b.2.cmp(&a.2)); // newest first

    println!("Backups in {}:\n", backups_dir.display());
    println!("  {:<50}  {:>10}  {}", "Name", "Size", "Date");
    println!("  {}", "-".repeat(80));
    for (name, size, date) in &entries {
        println!(
            "  {:<50}  {:>10}  {}",
            name,
            format_size(*size),
            date.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }
    println!("\n  Total: {} backup(s)", entries.len());

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
