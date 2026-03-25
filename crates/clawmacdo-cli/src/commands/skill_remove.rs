use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use std::path::PathBuf;

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

fn gateway_restart_cmd() -> &'static str {
    "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
     (systemctl --user -M openclaw@ restart openclaw-gateway.service 2>/dev/null || \
      runuser -l openclaw -c 'export XDG_RUNTIME_DIR=/run/user/$(id -u) && systemctl --user restart openclaw-gateway.service' 2>/dev/null || true) && \
     sleep 1 && \
     (runuser -l openclaw -c 'export XDG_RUNTIME_DIR=/run/user/$(id -u) && systemctl --user is-active openclaw-gateway.service' 2>/dev/null || echo 'unknown')"
}

/// Remove a deployed skill from the instance workspace and restart the gateway.
pub async fn remove(query: &str, skill_name: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let skill_dir = format!("{REMOTE_SKILLS_DIR}/{skill_name}");

    // Verify the skill exists before attempting removal
    let check_cmd = format!("[ -d '{skill_dir}' ] && echo 'exists' || echo 'not_found'");
    let check = ssh_as_openclaw_with_user_async(&ip, &key, &check_cmd, ssh_user).await?;
    if check.trim() == "not_found" {
        bail!(
            "Skill '{skill_name}' not found on {ip}.\nExpected: {skill_dir}\n\
             Run `clawmacdo skill-list --instance {query}` to see deployed skills."
        );
    }

    println!("Removing skill '{skill_name}' from {ip}...");
    println!("  Path: {skill_dir}");

    let rm_cmd = format!(
        "rm -rf '{skill_dir}' && echo 'removed OK' && {restart}",
        restart = gateway_restart_cmd()
    );
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &rm_cmd, ssh_user).await?;
    let output = output.trim();

    if output.contains("removed OK") {
        println!("  Skill directory deleted.");
    } else {
        println!("  {output}");
    }

    let gateway_status = output.lines().last().unwrap_or("unknown");
    println!("  gateway: {gateway_status}");
    println!("\nSkill '{skill_name}' removed from {ip}.");
    println!("The gateway has been restarted.");

    Ok(())
}

/// List all skill directories currently deployed on an instance.
pub async fn list(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;

    let cmd = format!(
        "if [ -d '{REMOTE_SKILLS_DIR}' ]; then \
           ls -1 '{REMOTE_SKILLS_DIR}' 2>/dev/null; \
         else \
           echo '__EMPTY__'; \
         fi"
    );
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let output = output.trim();

    println!("Skills deployed on {ip}:");
    println!("  Directory: {REMOTE_SKILLS_DIR}");
    println!();

    if output == "__EMPTY__" || output.is_empty() {
        println!("  (no skills deployed)");
        return Ok(());
    }

    // Also fetch gateway skill status for context
    let status_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw skills list 2>&1"
    );
    let gateway_skills = ssh_as_openclaw_with_user_async(&ip, &key, &status_cmd, ssh_user)
        .await
        .unwrap_or_default();

    for skill_dir_name in output.lines() {
        let skill_dir_name = skill_dir_name.trim();
        if skill_dir_name.is_empty() {
            continue;
        }
        // The gateway registers skills by their SKILL.md `name:` field, not the directory name.
        // Try to read the name from the remote SKILL.md so we can match against gateway output.
        let skill_md_path = format!("{REMOTE_SKILLS_DIR}/{skill_dir_name}/SKILL.md");
        let name_cmd = format!(
            "grep -m1 '^name:' '{skill_md_path}' 2>/dev/null | sed 's/^name:[[:space:]]*//' || echo '{skill_dir_name}'"
        );
        let skill_name = ssh_as_openclaw_with_user_async(&ip, &key, &name_cmd, ssh_user)
            .await
            .unwrap_or_else(|_| skill_dir_name.to_string());
        let skill_name = skill_name.trim();

        // Check if gateway recognises this skill by either directory name or SKILL.md name
        let active = gateway_skills.contains(skill_name) || gateway_skills.contains(skill_dir_name);
        let status = if active {
            "ready"
        } else {
            "deployed (not active in gateway)"
        };
        println!("  • {skill_dir_name}  (name: {skill_name})  [{status}]");
    }

    Ok(())
}

/// Check file ownership and permissions for a deployed skill.
///
/// Reports any files that are not owned by openclaw or have wrong permissions,
/// and optionally fixes them.
pub async fn check_permissions(query: &str, skill_name: &str, fix: bool) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let skill_dir = format!("{REMOTE_SKILLS_DIR}/{skill_name}");

    // Verify the skill exists
    let check_cmd = format!("[ -d '{skill_dir}' ] && echo 'exists' || echo 'not_found'");
    let check = ssh_as_openclaw_with_user_async(&ip, &key, &check_cmd, ssh_user).await?;
    if check.trim() == "not_found" {
        bail!("Skill '{skill_name}' not found on {ip}.\nExpected: {skill_dir}");
    }

    println!("Checking permissions for '{skill_name}' on {ip}...");
    println!("  Path: {skill_dir}");
    println!();

    // List all files/dirs with owner, group, and permissions using stat
    // Format: "PERMS OWNER GROUP PATH" — one line per entry, no duplicates
    let stat_cmd = format!(
        "find '{skill_dir}' \\( -type f -o -type d \\) | sort | \
         xargs stat -c '%A %U %G %n' 2>/dev/null"
    );
    let stat_out = ssh_as_openclaw_with_user_async(&ip, &key, &stat_cmd, ssh_user).await?;

    let mut issues = 0usize;
    for line in stat_out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // stat -c '%A %U %G %n' → "drwxr-xr-x openclaw openclaw /path/to/dir"
        let mut parts = line.splitn(4, ' ');
        let perms = parts.next().unwrap_or("");
        let owner = parts.next().unwrap_or("");
        let group = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or(line);

        // Show relative path for readability
        let display = name.strip_prefix(&format!("{skill_dir}/")).unwrap_or(name);

        let wrong_owner = owner != "openclaw" || group != "openclaw";
        let is_dir = perms.starts_with('d');
        let wrong_perms = if is_dir {
            perms != "drwxr-xr-x"
        } else {
            perms != "-rw-r--r--"
        };

        if wrong_owner || wrong_perms {
            issues += 1;
            let mut flags = Vec::new();
            if wrong_owner {
                flags.push(format!("owner={owner}:{group} (want openclaw:openclaw)"));
            }
            if wrong_perms {
                let want = if is_dir { "755" } else { "644" };
                flags.push(format!("perms={perms} (want {want})"));
            }
            println!("  ✗  {display}  — {}", flags.join(", "));
        } else {
            println!("  ✓  {display}  {perms}  {owner}:{group}");
        }
    }

    println!();
    if issues == 0 {
        println!("  All permissions OK.");
    } else {
        println!("  {issues} issue(s) found.");
        if fix {
            println!("  Fixing...");
            let fix_cmd = format!(
                "chown -R openclaw:openclaw '{skill_dir}' && \
                 find '{skill_dir}' -type f -exec chmod 644 {{}} \\; && \
                 find '{skill_dir}' -type d -exec chmod 755 {{}} \\; && \
                 echo 'fixed OK'"
            );
            let fix_out = ssh_as_openclaw_with_user_async(&ip, &key, &fix_cmd, ssh_user).await?;
            if fix_out.trim().contains("fixed OK") {
                println!("  Permissions corrected.");
            } else {
                println!("  {}", fix_out.trim());
            }
        } else {
            println!("  Run with --fix to correct ownership and permissions automatically.");
        }
    }

    Ok(())
}
