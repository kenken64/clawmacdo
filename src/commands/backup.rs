use crate::config;
use crate::ui;
use anyhow::{Context, Result};
use chrono::Utc;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::File;
use std::path::Path;

/// Create a backup archive of ~/.openclaw/ and the macOS LaunchAgent plist.
pub fn run() -> Result<()> {
    config::ensure_dirs()?;

    let openclaw_dir = config::openclaw_dir()?;
    let plist_path = config::launchagent_plist()?;
    let backups_dir = config::backups_dir()?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let archive_name = format!("openclaw_backup_{timestamp}.tar.gz");
    let archive_path = backups_dir.join(&archive_name);

    let sp = ui::spinner("Creating backup archive...");

    let file = File::create(&archive_path)
        .with_context(|| format!("Failed to create {}", archive_path.display()))?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = tar::Builder::new(enc);

    // Add ~/.openclaw/ directory
    if openclaw_dir.exists() {
        tar.append_dir_all("openclaw", &openclaw_dir)
            .with_context(|| format!("Failed to add {} to archive", openclaw_dir.display()))?;
    } else {
        sp.println(format!(
            "  Warning: {} does not exist, skipping",
            openclaw_dir.display()
        ));
    }

    // Add LaunchAgent plist if it exists (macOS only)
    if plist_path.exists() {
        tar.append_path_with_name(&plist_path, "launchagent/ai.openclaw.gateway.plist")
            .with_context(|| format!("Failed to add {} to archive", plist_path.display()))?;
    }

    let enc = tar.into_inner()?;
    enc.finish()?;

    sp.finish_and_clear();

    let size = std::fs::metadata(&archive_path)?.len();
    println!("Backup created: {}", archive_path.display());
    println!("Size: {}", format_size(size));

    // List contents
    println!("\nArchive contents:");
    let count = list_tar_contents(&archive_path)?;
    if count == 0 {
        println!("  (empty archive — no files found to back up)");
    }

    Ok(())
}

fn list_tar_contents(path: &Path) -> Result<usize> {
    let file = File::open(path)?;
    let dec = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(dec);
    let mut count = 0;
    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?;
        let size = entry.size();
        println!("  {:>8}  {}", format_size(size), path.display());
        count += 1;
    }
    Ok(count)
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
