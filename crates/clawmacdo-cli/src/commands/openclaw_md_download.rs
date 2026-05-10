use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};

const BOOTSTRAP_FILES: &[&str] = &[
    "AGENTS.md",
    "SOUL.md",
    "IDENTITY.md",
    "USER.md",
    "TOOLS.md",
    "HEARTBEAT.md",
    "BOOTSTRAP.md",
    "llm_wiki.md",
];

pub struct OpenclawMdDownloadParams {
    pub instance: String,
    pub agent: String,
    pub output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RemoteArchiveManifest {
    workspace: String,
    files: Vec<RemoteMdFile>,
    #[serde(default)]
    missing: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RemoteMdFile {
    name: String,
    data: String,
    size: u64,
}

struct ZipEntry {
    name: String,
    data: Vec<u8>,
    crc32: u32,
}

struct CentralDirectoryEntry {
    name: String,
    crc32: u32,
    size: u32,
    offset: u32,
}

/// Look up a deploy record by hostname, IP, or deploy ID.
/// Returns (ip, ssh_key_path, provider).
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
        Some("azure") => "azureuser",
        _ => "root",
    }
}

fn clean_agent_id(value: &str) -> Result<String> {
    let agent = value.trim();
    if agent.is_empty() {
        bail!("--agent cannot be empty.");
    }
    if agent.len() > 80 {
        bail!("--agent must be 80 bytes or fewer.");
    }
    if !agent
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        bail!("--agent may only contain letters, numbers, dots, underscores, and hyphens.");
    }
    Ok(agent.to_string())
}

fn js_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn build_collect_cmd(agent: &str) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let agent = js_string(agent)?;
    let files_json = serde_json::to_string(BOOTSTRAP_FILES)?;

    Ok(format!(
        r#"export HOME="{home}" && \
         node <<'NODE'
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '{home}';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = {agent};
const bootstrapFiles = {files_json};

function readJson(file) {{
  try {{
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  }} catch (_) {{
    return {{}};
  }}
}}

function expandWorkspace(raw) {{
  let value = typeof raw === 'string' && raw.trim()
    ? raw.trim()
    : path.join(home, '.openclaw', 'workspace');
  if (value === '~') value = home;
  if (value.startsWith('~/')) value = path.join(home, value.slice(2));
  if (value.startsWith('$HOME/')) value = path.join(home, value.slice(6));
  if (value.startsWith('${{HOME}}/')) value = path.join(home, value.slice(8));
  if (!path.isAbsolute(value)) value = path.join(home, value);
  return path.normalize(value);
}}

function addFile(result, workspace, archiveName, filePath) {{
  const data = fs.readFileSync(filePath);
  result.files.push({{
    name: archiveName.replace(/\\/g, '/'),
    data: data.toString('base64'),
    size: data.length
  }});
}}

function walkMemory(result, workspace, dir, prefix) {{
  if (!fs.existsSync(dir)) return;
  for (const item of fs.readdirSync(dir, {{ withFileTypes: true }})) {{
    const itemPath = path.join(dir, item.name);
    const archiveName = prefix ? `${{prefix}}/${{item.name}}` : item.name;
    if (item.isDirectory()) {{
      walkMemory(result, workspace, itemPath, archiveName);
    }} else if (item.isFile() && item.name.toLowerCase().endsWith('.md')) {{
      addFile(result, workspace, archiveName, itemPath);
    }}
  }}
}}

const cfg = readJson(configPath);
const agents = cfg.agents || {{}};
const list = Array.isArray(agents.list) ? agents.list : [];
const agent = list.find((item) => item && item.id === agentId);
const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : agents.defaults && agents.defaults.workspace
);

const result = {{ workspace, files: [], missing: [] }};
for (const name of bootstrapFiles) {{
  const filePath = path.join(workspace, name);
  if (fs.existsSync(filePath) && fs.statSync(filePath).isFile()) {{
    addFile(result, workspace, name, filePath);
  }} else {{
    result.missing.push(name);
  }}
}}

walkMemory(result, workspace, path.join(workspace, 'memory'), 'memory');
result.files.sort((a, b) => a.name.localeCompare(b.name));
result.missing.sort((a, b) => a.localeCompare(b));
console.log(JSON.stringify(result));
NODE"#,
        home = home,
        agent = agent,
        files_json = files_json,
    ))
}

fn resolve_output_path(output: &Path) -> Result<PathBuf> {
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let default_name = format!("openclaw-md-files-{timestamp}.zip");
    let is_zip = output
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false);

    let path = if output.exists() && output.is_dir() {
        output.join(default_name)
    } else if is_zip {
        output.to_path_buf()
    } else {
        output.join(default_name)
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    Ok(path)
}

fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn to_u16(value: usize, label: &str) -> Result<u16> {
    u16::try_from(value).with_context(|| format!("{label} is too large for ZIP32"))
}

fn to_u32(value: usize, label: &str) -> Result<u32> {
    u32::try_from(value).with_context(|| format!("{label} is too large for ZIP32"))
}

fn write_zip(path: &Path, entries: &[ZipEntry]) -> Result<()> {
    let mut out = Vec::new();
    let mut central_entries = Vec::new();
    let mod_time = 0u16;
    let mod_date = 33u16; // 1980-01-01
    let flags = 0x0800u16; // UTF-8 names

    for entry in entries {
        let name = entry.name.as_bytes();
        let size = to_u32(entry.data.len(), &entry.name)?;
        let offset = to_u32(out.len(), "ZIP local header offset")?;

        write_u32(&mut out, 0x0403_4b50);
        write_u16(&mut out, 20);
        write_u16(&mut out, flags);
        write_u16(&mut out, 0);
        write_u16(&mut out, mod_time);
        write_u16(&mut out, mod_date);
        write_u32(&mut out, entry.crc32);
        write_u32(&mut out, size);
        write_u32(&mut out, size);
        write_u16(&mut out, to_u16(name.len(), "ZIP file name")?);
        write_u16(&mut out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(&entry.data);

        central_entries.push(CentralDirectoryEntry {
            name: entry.name.clone(),
            crc32: entry.crc32,
            size,
            offset,
        });
    }

    let central_offset = to_u32(out.len(), "ZIP central directory offset")?;
    for entry in &central_entries {
        let name = entry.name.as_bytes();

        write_u32(&mut out, 0x0201_4b50);
        write_u16(&mut out, 20);
        write_u16(&mut out, 20);
        write_u16(&mut out, flags);
        write_u16(&mut out, 0);
        write_u16(&mut out, mod_time);
        write_u16(&mut out, mod_date);
        write_u32(&mut out, entry.crc32);
        write_u32(&mut out, entry.size);
        write_u32(&mut out, entry.size);
        write_u16(&mut out, to_u16(name.len(), "ZIP file name")?);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u32(&mut out, 0);
        write_u32(&mut out, entry.offset);
        out.extend_from_slice(name);
    }
    let central_size = to_u32(out.len(), "ZIP output length")? - central_offset;
    let entry_count = to_u16(central_entries.len(), "ZIP entry count")?;

    write_u32(&mut out, 0x0605_4b50);
    write_u16(&mut out, 0);
    write_u16(&mut out, 0);
    write_u16(&mut out, entry_count);
    write_u16(&mut out, entry_count);
    write_u32(&mut out, central_size);
    write_u32(&mut out, central_offset);
    write_u16(&mut out, 0);

    let mut file = std::fs::File::create(path)?;
    file.write_all(&out)?;
    Ok(())
}

pub async fn run(params: OpenclawMdDownloadParams) -> Result<()> {
    let instance = params.instance.trim();
    if instance.is_empty() {
        bail!("--instance cannot be empty.");
    }

    let agent = clean_agent_id(&params.agent)?;
    let (ip, key, provider) = find_deploy_record(instance)?;
    let ssh_user = ssh_user_for_provider(&provider);

    println!("Collecting OpenClaw Markdown files from {ip}...");
    let cmd = build_collect_cmd(&agent)?;
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let manifest: RemoteArchiveManifest =
        serde_json::from_str(output.trim()).context("Failed to parse remote Markdown manifest")?;

    if manifest.files.is_empty() {
        bail!(
            "No Markdown context files were found in workspace {}.",
            manifest.workspace
        );
    }

    let mut entries = Vec::new();
    let mut total_size = 0u64;
    for file in &manifest.files {
        let data = BASE64
            .decode(&file.data)
            .with_context(|| format!("Failed to decode {}", file.name))?;
        if data.len() as u64 != file.size {
            bail!(
                "Remote size mismatch for {}: expected {}, decoded {}",
                file.name,
                file.size,
                data.len()
            );
        }
        total_size += file.size;
        entries.push(ZipEntry {
            name: file.name.clone(),
            crc32: crc32(&data),
            data,
        });
    }

    let zip_path = resolve_output_path(&params.output)?;
    write_zip(&zip_path, &entries)?;

    println!("  Workspace: {}", manifest.workspace);
    println!("  Files:     {}", entries.len());
    println!("  Size:      {}", format_size(total_size));
    if !manifest.missing.is_empty() {
        println!("  Missing:   {}", manifest.missing.join(", "));
    }
    println!();
    println!(
        "OpenClaw Markdown ZIP downloaded to: {}",
        zip_path.display()
    );

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
