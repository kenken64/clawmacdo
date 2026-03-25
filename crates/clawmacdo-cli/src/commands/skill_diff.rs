use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const REMOTE_SKILLS_DIR: &str = "/home/openclaw/.openclaw/workspace/skills";

fn find_deploy_record(query: &str) -> Result<(String, PathBuf, Option<String>)> {
    let deploys_dir = config::deploys_dir()?;
    if !deploys_dir.exists() {
        bail!("No deploy records found. Deploy an instance first.");
    }
    for entry in std::fs::read_dir(&deploys_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let contents = std::fs::read_to_string(&path)?;
        let record: config::DeployRecord = match serde_json::from_str(&contents) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if record.id == query || record.hostname == query || record.ip_address == query {
            let provider = record.provider.map(|p| p.to_string());
            return Ok((
                record.ip_address,
                PathBuf::from(record.ssh_key_path),
                provider,
            ));
        }
    }
    bail!("No deploy record found for '{query}'. Use a deploy ID, hostname, or IP address.");
}

fn ssh_user_for_provider(provider: &Option<String>) -> &'static str {
    match provider.as_deref() {
        Some("lightsail") => "ubuntu",
        _ => "root",
    }
}

/// Collect local files from a skill directory: path relative to skill root → (abs_path, md5).
fn local_checksums(skill_dir: &Path) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    collect_files(skill_dir, skill_dir, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path)?;
            let digest = hex::encode(Sha256::digest(&bytes));
            out.push((rel, digest));
        }
    }
    Ok(())
}

/// Fetch remote checksums for a named skill on an OpenClaw instance.
/// Returns Vec<(relative_path, md5_hex)> sorted by path.
async fn remote_checksums(
    ip: &str,
    key: &Path,
    ssh_user: &str,
    skill_name: &str,
) -> Result<Vec<(String, String)>> {
    let remote_dir = format!("{REMOTE_SKILLS_DIR}/{skill_name}");
    let cmd = format!(
        "if [ -d {remote_dir} ]; then \
           find {remote_dir} -type f | sort | while read f; do \
             echo \"${{f#{remote_dir}/}} $(sha256sum $f | cut -d' ' -f1)\"; \
           done; \
         else \
           echo '__NOT_FOUND__'; \
         fi"
    );
    let output = ssh_as_openclaw_with_user_async(ip, key, &cmd, ssh_user).await?;
    let output = output.trim();

    if output == "__NOT_FOUND__" {
        bail!("Skill '{skill_name}' not found on instance {ip}.\nExpected: {remote_dir}");
    }

    let mut entries = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let rel = parts.next().unwrap_or("").to_string();
        let hash = parts.next().unwrap_or("").to_string();
        if !rel.is_empty() && !hash.is_empty() {
            entries.push((rel, hash));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Compare local skill directory against the deployed skill on an instance.
///
/// Prints a drift report: files that are in-sync, modified, added, or missing.
pub async fn diff(query: &str, skill_dir: &Path) -> Result<()> {
    if !skill_dir.exists() {
        bail!("Local skill directory not found: {}", skill_dir.display());
    }
    let skill_name = skill_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Checking skill drift for '{skill_name}' on {ip}...");
    println!("  Local:  {}", skill_dir.display());
    println!("  Remote: {REMOTE_SKILLS_DIR}/{skill_name}");
    println!();

    let local = local_checksums(skill_dir)?;
    let remote = remote_checksums(&ip, &key, ssh_user, skill_name).await?;

    // Build lookup maps
    let local_map: std::collections::HashMap<&str, &str> = local
        .iter()
        .map(|(p, h)| (p.as_str(), h.as_str()))
        .collect();
    let remote_map: std::collections::HashMap<&str, &str> = remote
        .iter()
        .map(|(p, h)| (p.as_str(), h.as_str()))
        .collect();

    let mut in_sync = 0usize;
    let mut drifted = false;

    // Files in local
    let mut all_paths: Vec<&str> = local_map
        .keys()
        .copied()
        .chain(remote_map.keys().copied())
        .collect();
    all_paths.sort_unstable();
    all_paths.dedup();

    for path in &all_paths {
        match (local_map.get(path), remote_map.get(path)) {
            (Some(lh), Some(rh)) if lh == rh => {
                println!("  ✓  {path}");
                in_sync += 1;
            }
            (Some(_), Some(_)) => {
                println!("  ≠  {path}  ← MODIFIED locally (needs redeploy)");
                drifted = true;
            }
            (Some(_), None) => {
                println!("  +  {path}  ← NEW locally (not on instance)");
                drifted = true;
            }
            (None, Some(_)) => {
                println!("  -  {path}  ← ONLY on instance (deleted locally)");
                drifted = true;
            }
            _ => {}
        }
    }

    println!();
    println!("  {in_sync}/{} files in sync", all_paths.len());

    if drifted {
        println!();
        println!("  Drift detected. Run `clawmacdo skill-deploy` to sync.");
    } else {
        println!("  No drift — instance is up to date.");
    }

    // Also show gateway detection status
    let home = config::OPENCLAW_HOME;
    let status_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) && \
         openclaw skills list 2>&1 | grep -i '{skill_name}\\|{skill_name}' | head -3"
    );
    if let Ok(status) = ssh_as_openclaw_with_user_async(&ip, &key, &status_cmd, ssh_user).await {
        let s = status.trim();
        if !s.is_empty() {
            println!();
            println!("  Gateway status:");
            println!("  {s}");
        }
    }

    Ok(())
}
