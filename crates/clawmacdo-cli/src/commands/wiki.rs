use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use serde::Deserialize;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_MARKDOWN_BYTES: u64 = 5 * 1024 * 1024;

pub struct WikiTreeParams {
    pub instance: String,
    pub agent: String,
    pub project: String,
    pub json: bool,
}

pub struct WikiIndexParams {
    pub instance: String,
    pub agent: String,
    pub project: String,
    pub json: bool,
}

pub struct WikiReadParams {
    pub instance: String,
    pub agent: String,
    pub path: String,
    pub json: bool,
}

pub struct WikiWriteParams {
    pub instance: String,
    pub agent: String,
    pub path: String,
    pub content_file: PathBuf,
    pub base_sha: String,
    pub json: bool,
}

pub struct WikiExportParams {
    pub instance: String,
    pub agent: String,
    pub project: String,
    pub output: PathBuf,
    pub json: bool,
}

pub struct WikiDeleteParams {
    pub instance: String,
    pub agent: String,
    pub project: String,
    pub json: bool,
}

#[derive(Debug, Deserialize)]
struct RemoteManifest {
    workspace: String,
    project: String,
    files: Vec<RemoteWikiFile>,
}

#[derive(Debug, Deserialize)]
struct RemoteWikiFile {
    path: String,
    size: u64,
    sha256: String,
    #[serde(default)]
    mtime: String,
    #[serde(default)]
    data: Option<String>,
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

fn clean_instance(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--instance cannot be empty.");
    }
    if value.len() > 255 {
        bail!("--instance must be 255 bytes or fewer.");
    }
    if value.chars().any(char::is_control) {
        bail!("--instance cannot contain control characters.");
    }
    Ok(value.to_string())
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

fn clean_project_slug(value: &str) -> Result<String> {
    let project = value.trim();
    if project.is_empty() {
        bail!("--project cannot be empty.");
    }
    if project.len() > 120 {
        bail!("--project must be 120 bytes or fewer.");
    }
    if project == "." || project == ".." {
        bail!("--project cannot be '.' or '..'.");
    }
    if !project
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        bail!("--project may only contain letters, numbers, dots, underscores, and hyphens.");
    }
    Ok(project.to_string())
}

fn clean_deletable_wiki_project_slug(value: &str) -> Result<String> {
    let project = clean_project_slug(value)?;
    let Some(suffix) = project.strip_prefix("wiki-") else {
        bail!("--project must start with 'wiki-' for wiki-delete.");
    };
    if suffix.is_empty() {
        bail!("--project must include a suffix after 'wiki-'.");
    }
    if suffix.starts_with('.') || project.contains("..") {
        bail!("--project cannot contain hidden or parent-directory segments.");
    }
    Ok(project)
}

fn clean_relative_markdown_path(value: &str) -> Result<String> {
    let path = value.trim();
    if path.is_empty() {
        bail!("--path cannot be empty.");
    }
    if path.len() > 512 {
        bail!("--path must be 512 bytes or fewer.");
    }
    if path.starts_with('/') || path.starts_with('~') {
        bail!("--path must be relative to the OpenClaw workspace.");
    }
    if path.contains('\\') {
        bail!("--path must use '/' separators.");
    }
    if path.chars().any(char::is_control) {
        bail!("--path cannot contain control characters.");
    }
    if !path.to_ascii_lowercase().ends_with(".md") {
        bail!("--path must point to a Markdown .md file.");
    }

    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            bail!("--path cannot contain empty, '.', or '..' path segments.");
        }
    }

    Ok(path.to_string())
}

fn clean_base_sha(value: &str) -> Result<String> {
    let sha = value.trim();
    if sha.eq_ignore_ascii_case("new") {
        return Ok("NEW".to_string());
    }
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("--base-sha must be a 64-character SHA-256 hex digest, or NEW when creating a new file.");
    }
    Ok(sha.to_ascii_lowercase())
}

fn clean_content_file(path: PathBuf) -> Result<PathBuf> {
    if !path.exists() {
        bail!("--content-file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("--content-file must point to a file: {}", path.display());
    }
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() > MAX_MARKDOWN_BYTES {
        bail!("--content-file must be 5 MiB or smaller.");
    }
    path.canonicalize()
        .with_context(|| format!("Failed to resolve --content-file path {}", path.display()))
}

fn js_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn remote_json_value(output: &str, context: &str) -> Result<Value> {
    serde_json::from_str(output.trim()).with_context(|| format!("Failed to parse {context} JSON"))
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn handle_remote_status(value: &Value, json: bool) -> Result<()> {
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        if json {
            print_json(value)?;
        }
        let message = value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("remote wiki command failed");
        bail!("{message}");
    }
    Ok(())
}

fn build_collect_cmd(
    agent: &str,
    project: &str,
    include_content: bool,
    include_index: bool,
) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let mut template = r#"set -e
export HOME="__HOME__"
export PATH="__HOME__/.local/bin:__HOME__/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"

node <<'NODE'
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const home = process.env.HOME || '__HOME__';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = __AGENT_JSON__;
const project = __PROJECT_JSON__;
const includeContent = __INCLUDE_CONTENT__;
const includeIndex = __INCLUDE_INDEX__;

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (_) {
    return {};
  }
}

function expandWorkspace(raw) {
  let value = typeof raw === 'string' && raw.trim()
    ? raw.trim()
    : path.join(home, '.openclaw', 'workspace');
  if (value === '~') value = home;
  if (value.startsWith('~/')) value = path.join(home, value.slice(2));
  if (value.startsWith('$HOME/')) value = path.join(home, value.slice(6));
  if (value.startsWith('${HOME}/')) value = path.join(home, value.slice(8));
  if (!path.isAbsolute(value)) value = path.join(home, value);
  return path.normalize(value);
}

function isWithin(root, target) {
  const rel = path.relative(root, target);
  return rel === '' || (!!rel && !rel.startsWith('..') && !path.isAbsolute(rel));
}

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function normalizeRel(value) {
  return value.split(path.sep).join('/');
}

function extractIndex(file) {
  if (!includeIndex) return {};
  const text = fs.readFileSync(file, 'utf8');
  const headings = [];
  const tags = new Set();
  const links = new Set();

  for (const line of text.split(/\r?\n/)) {
    const heading = line.match(/^(#{1,6})\s+(.+?)\s*#*\s*$/);
    if (heading) {
      headings.push({ level: heading[1].length, text: heading[2].trim() });
    }
    for (const tag of line.matchAll(/(^|\s)#([A-Za-z0-9][A-Za-z0-9_-]{1,63})\b/g)) {
      tags.add(tag[2]);
    }
    for (const link of line.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
      links.add(link[1].trim());
    }
    for (const link of line.matchAll(/\[\[([^\]]+)\]\]/g)) {
      links.add(link[1].trim());
    }
  }

  return {
    headings,
    tags: Array.from(tags).sort((a, b) => a.localeCompare(b)),
    links: Array.from(links).sort((a, b) => a.localeCompare(b))
  };
}

function addFile(result, workspaceReal, file) {
  const stat = fs.statSync(file);
  const rel = normalizeRel(path.relative(workspaceReal, fs.realpathSync(file)));
  if (!rel || rel.startsWith('../') || !rel.toLowerCase().endsWith('.md')) return;

  const item = {
    path: rel,
    size: stat.size,
    mtime: stat.mtime.toISOString(),
    sha256: sha256(file),
    ...extractIndex(file)
  };
  if (includeContent) {
    item.data = fs.readFileSync(file).toString('base64');
  }
  result.files.push(item);
}

function walk(result, workspaceReal, dir) {
  if (!fs.existsSync(dir)) return;
  const dirStat = fs.lstatSync(dir);
  if (!dirStat.isDirectory() || dirStat.isSymbolicLink()) return;
  const dirReal = fs.realpathSync(dir);
  if (!isWithin(workspaceReal, dirReal)) return;

  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const entryPath = path.join(dir, entry.name);
    if (entry.isSymbolicLink()) continue;
    if (entry.isDirectory()) {
      walk(result, workspaceReal, entryPath);
    } else if (entry.isFile() && entry.name.toLowerCase().endsWith('.md')) {
      addFile(result, workspaceReal, entryPath);
    }
  }
}

const cfg = readJson(configPath);
const agents = cfg.agents || {};
const list = Array.isArray(agents.list) ? agents.list : [];
const agent = list.find((item) => item && item.id === agentId);
const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : agents.defaults && agents.defaults.workspace
);

if (!fs.existsSync(workspace) || !fs.statSync(workspace).isDirectory()) {
  throw new Error(`OpenClaw workspace not found: ${workspace}`);
}

const workspaceReal = fs.realpathSync(workspace);
const result = {
  ok: true,
  workspace: workspaceReal,
  project,
  roots: [],
  files: []
};

const rootMd = path.join(workspaceReal, `${project}.md`);
if (fs.existsSync(rootMd) && fs.lstatSync(rootMd).isFile() && !fs.lstatSync(rootMd).isSymbolicLink()) {
  result.roots.push(normalizeRel(path.relative(workspaceReal, rootMd)));
  addFile(result, workspaceReal, rootMd);
}

const projectDir = path.join(workspaceReal, project);
if (fs.existsSync(projectDir) && fs.lstatSync(projectDir).isDirectory() && !fs.lstatSync(projectDir).isSymbolicLink()) {
  result.roots.push(normalizeRel(path.relative(workspaceReal, projectDir)));
  walk(result, workspaceReal, projectDir);
}

result.roots.sort((a, b) => a.localeCompare(b));
result.files.sort((a, b) => a.path.localeCompare(b.path));
console.log(JSON.stringify(result));
NODE
"#
    .to_string();

    template = template.replace("__HOME__", home);
    template = template.replace("__AGENT_JSON__", &js_string(agent)?);
    template = template.replace("__PROJECT_JSON__", &js_string(project)?);
    template = template.replace(
        "__INCLUDE_CONTENT__",
        if include_content { "true" } else { "false" },
    );
    template = template.replace(
        "__INCLUDE_INDEX__",
        if include_index { "true" } else { "false" },
    );
    Ok(template)
}

fn build_read_cmd(agent: &str, rel_path: &str) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let mut template = r#"set -e
export HOME="__HOME__"
export PATH="__HOME__/.local/bin:__HOME__/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"

node <<'NODE'
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const home = process.env.HOME || '__HOME__';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = __AGENT_JSON__;
const relPath = __PATH_JSON__;

function fail(code, message, extra = {}) {
  console.log(JSON.stringify({ ok: false, error: { code, message }, ...extra }));
  process.exit(0);
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (_) {
    return {};
  }
}

function expandWorkspace(raw) {
  let value = typeof raw === 'string' && raw.trim()
    ? raw.trim()
    : path.join(home, '.openclaw', 'workspace');
  if (value === '~') value = home;
  if (value.startsWith('~/')) value = path.join(home, value.slice(2));
  if (value.startsWith('$HOME/')) value = path.join(home, value.slice(6));
  if (value.startsWith('${HOME}/')) value = path.join(home, value.slice(8));
  if (!path.isAbsolute(value)) value = path.join(home, value);
  return path.normalize(value);
}

function isWithin(root, target) {
  const rel = path.relative(root, target);
  return rel === '' || (!!rel && !rel.startsWith('..') && !path.isAbsolute(rel));
}

function sha256Bytes(data) {
  return crypto.createHash('sha256').update(data).digest('hex');
}

const cfg = readJson(configPath);
const agents = cfg.agents || {};
const list = Array.isArray(agents.list) ? agents.list : [];
const agent = list.find((item) => item && item.id === agentId);
const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : agents.defaults && agents.defaults.workspace
);

if (!fs.existsSync(workspace) || !fs.statSync(workspace).isDirectory()) {
  fail('workspace_not_found', `OpenClaw workspace not found: ${workspace}`);
}
if (!relPath.toLowerCase().endsWith('.md')) {
  fail('invalid_path', 'Only Markdown .md files can be read.');
}

const workspaceReal = fs.realpathSync(workspace);
const target = path.resolve(workspaceReal, relPath);
let stat;
try {
  stat = fs.lstatSync(target);
} catch (_) {
  fail('not_found', `Markdown file not found: ${relPath}`, { path: relPath });
}
if (stat.isSymbolicLink() || !stat.isFile()) {
  fail('invalid_path', `Path is not a regular Markdown file: ${relPath}`, { path: relPath });
}
const targetReal = fs.realpathSync(target);
if (!isWithin(workspaceReal, targetReal)) {
  fail('path_escape', 'Path escapes the OpenClaw workspace allowlist.', { path: relPath });
}

const data = fs.readFileSync(targetReal);
console.log(JSON.stringify({
  ok: true,
  workspace: workspaceReal,
  path: relPath,
  size: data.length,
  mtime: stat.mtime.toISOString(),
  sha256: sha256Bytes(data),
  content: data.toString('utf8')
}));
NODE
"#
    .to_string();

    template = template.replace("__HOME__", home);
    template = template.replace("__AGENT_JSON__", &js_string(agent)?);
    template = template.replace("__PATH_JSON__", &js_string(rel_path)?);
    Ok(template)
}

fn build_write_cmd(
    agent: &str,
    rel_path: &str,
    base_sha: &str,
    upload_tmp: &str,
) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let mut template = r#"set -e
export HOME="__HOME__"
export PATH="__HOME__/.local/bin:__HOME__/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"

node <<'NODE'
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const home = process.env.HOME || '__HOME__';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = __AGENT_JSON__;
const relPath = __PATH_JSON__;
const baseSha = __BASE_SHA_JSON__;
const uploadTmp = __UPLOAD_TMP_JSON__;
const maxBytes = __MAX_BYTES__;

function fail(code, message, extra = {}) {
  console.log(JSON.stringify({ ok: false, error: { code, message }, ...extra }));
  process.exit(0);
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (_) {
    return {};
  }
}

function expandWorkspace(raw) {
  let value = typeof raw === 'string' && raw.trim()
    ? raw.trim()
    : path.join(home, '.openclaw', 'workspace');
  if (value === '~') value = home;
  if (value.startsWith('~/')) value = path.join(home, value.slice(2));
  if (value.startsWith('$HOME/')) value = path.join(home, value.slice(6));
  if (value.startsWith('${HOME}/')) value = path.join(home, value.slice(8));
  if (!path.isAbsolute(value)) value = path.join(home, value);
  return path.normalize(value);
}

function isWithin(root, target) {
  const rel = path.relative(root, target);
  return rel === '' || (!!rel && !rel.startsWith('..') && !path.isAbsolute(rel));
}

function sha256Bytes(data) {
  return crypto.createHash('sha256').update(data).digest('hex');
}

const cfg = readJson(configPath);
const agents = cfg.agents || {};
const list = Array.isArray(agents.list) ? agents.list : [];
const agent = list.find((item) => item && item.id === agentId);
const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : agents.defaults && agents.defaults.workspace
);

if (!fs.existsSync(workspace) || !fs.statSync(workspace).isDirectory()) {
  fail('workspace_not_found', `OpenClaw workspace not found: ${workspace}`);
}
if (!relPath.toLowerCase().endsWith('.md')) {
  fail('invalid_path', 'Only Markdown .md files can be written.');
}
if (!fs.existsSync(uploadTmp) || !fs.lstatSync(uploadTmp).isFile()) {
  fail('upload_not_found', 'Uploaded Markdown content was not found on the instance.');
}

const workspaceReal = fs.realpathSync(workspace);
const target = path.resolve(workspaceReal, relPath);
if (!isWithin(workspaceReal, target)) {
  fail('path_escape', 'Path escapes the OpenClaw workspace allowlist.', { path: relPath });
}

const content = fs.readFileSync(uploadTmp);
if (content.length > maxBytes) {
  fail('too_large', `Markdown content exceeds ${maxBytes} bytes.`, { path: relPath });
}

const parent = path.dirname(target);
fs.mkdirSync(parent, { recursive: true });
const parentReal = fs.realpathSync(parent);
if (!isWithin(workspaceReal, parentReal)) {
  fail('path_escape', 'Parent directory escapes the OpenClaw workspace allowlist.', { path: relPath });
}

let previousSha = null;
let created = false;
if (fs.existsSync(target)) {
  const stat = fs.lstatSync(target);
  if (stat.isSymbolicLink() || !stat.isFile()) {
    fail('invalid_path', `Path is not a regular Markdown file: ${relPath}`, { path: relPath });
  }
  const targetReal = fs.realpathSync(target);
  if (!isWithin(workspaceReal, targetReal)) {
    fail('path_escape', 'Path escapes the OpenClaw workspace allowlist.', { path: relPath });
  }
  previousSha = sha256Bytes(fs.readFileSync(targetReal));
  if (baseSha === 'NEW') {
    fail('already_exists', 'File already exists; pass the current sha256 as --base-sha to update it.', {
      path: relPath,
      current_sha: previousSha
    });
  }
  if (previousSha !== baseSha.toLowerCase()) {
    fail('sha_mismatch', 'File changed since it was read; refresh and retry with the current sha256.', {
      path: relPath,
      base_sha: baseSha,
      current_sha: previousSha
    });
  }
} else {
  created = true;
  if (baseSha !== 'NEW') {
    fail('not_found', 'File does not exist; pass --base-sha NEW to create it.', { path: relPath });
  }
}

fs.writeFileSync(target, content, { mode: 0o644 });
fs.chmodSync(target, 0o644);
try { fs.unlinkSync(uploadTmp); } catch (_) {}

const stat = fs.statSync(target);
const nextData = fs.readFileSync(target);
console.log(JSON.stringify({
  ok: true,
  workspace: workspaceReal,
  path: relPath,
  created,
  previous_sha: previousSha,
  sha256: sha256Bytes(nextData),
  size: nextData.length,
  mtime: stat.mtime.toISOString()
}));
NODE
"#
    .to_string();

    template = template.replace("__HOME__", home);
    template = template.replace("__AGENT_JSON__", &js_string(agent)?);
    template = template.replace("__PATH_JSON__", &js_string(rel_path)?);
    template = template.replace("__BASE_SHA_JSON__", &js_string(base_sha)?);
    template = template.replace("__UPLOAD_TMP_JSON__", &js_string(upload_tmp)?);
    template = template.replace("__MAX_BYTES__", &MAX_MARKDOWN_BYTES.to_string());
    Ok(template)
}

fn build_delete_cmd(agent: &str, project: &str) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let mut template = r#"set -e
export HOME="__HOME__"
export PATH="__HOME__/.local/bin:__HOME__/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"

node <<'NODE'
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '__HOME__';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = __AGENT_JSON__;
const project = __PROJECT_JSON__;

function fail(code, message, extra = {}) {
  console.log(JSON.stringify({ ok: false, error: { code, message }, ...extra }));
  process.exit(0);
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (_) {
    return {};
  }
}

function expandWorkspace(raw) {
  let value = typeof raw === 'string' && raw.trim()
    ? raw.trim()
    : path.join(home, '.openclaw', 'workspace');
  if (value === '~') value = home;
  if (value.startsWith('~/')) value = path.join(home, value.slice(2));
  if (value.startsWith('$HOME/')) value = path.join(home, value.slice(6));
  if (value.startsWith('${HOME}/')) value = path.join(home, value.slice(8));
  if (!path.isAbsolute(value)) value = path.join(home, value);
  return path.normalize(value);
}

function isWithin(root, target) {
  const rel = path.relative(root, target);
  return rel === '' || (!!rel && !rel.startsWith('..') && !path.isAbsolute(rel));
}

if (!/^wiki-[A-Za-z0-9][A-Za-z0-9._-]{0,114}$/.test(project) || project.includes('..')) {
  fail('invalid_project', "wiki-delete only accepts direct project slugs beginning with 'wiki-'.", { project });
}

const cfg = readJson(configPath);
const agents = cfg.agents || {};
const list = Array.isArray(agents.list) ? agents.list : [];
const agent = list.find((item) => item && item.id === agentId);
const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : agents.defaults && agents.defaults.workspace
);

if (!fs.existsSync(workspace) || !fs.statSync(workspace).isDirectory()) {
  fail('workspace_not_found', `OpenClaw workspace not found: ${workspace}`);
}

const workspaceReal = fs.realpathSync(workspace);
const target = path.resolve(workspaceReal, project);
if (target === workspaceReal || path.dirname(target) !== workspaceReal || !isWithin(workspaceReal, target)) {
  fail('path_escape', 'Resolved project directory is not a direct child of the OpenClaw workspace.', {
    workspace: workspaceReal,
    project,
    path: target
  });
}

let deleted = false;
let existed = false;
try {
  const stat = fs.lstatSync(target);
  existed = true;
  if (stat.isSymbolicLink() || !stat.isDirectory()) {
    fail('invalid_path', 'wiki-delete can only remove a regular project directory.', {
      workspace: workspaceReal,
      project,
      path: target
    });
  }
  const targetReal = fs.realpathSync(target);
  if (targetReal === workspaceReal || path.dirname(targetReal) !== workspaceReal || !isWithin(workspaceReal, targetReal)) {
    fail('path_escape', 'Resolved project directory escapes the OpenClaw workspace allowlist.', {
      workspace: workspaceReal,
      project,
      path: targetReal
    });
  }
  fs.rmSync(targetReal, { recursive: true, force: false });
  deleted = true;
} catch (error) {
  if (error && error.code === 'ENOENT') {
    existed = false;
  } else {
    throw error;
  }
}

console.log(JSON.stringify({
  ok: true,
  workspace: workspaceReal,
  project,
  path: target,
  existed,
  deleted
}));
NODE
"#
    .to_string();

    template = template.replace("__HOME__", home);
    template = template.replace("__AGENT_JSON__", &js_string(agent)?);
    template = template.replace("__PROJECT_JSON__", &js_string(project)?);
    Ok(template)
}

fn manifest_from_value(value: Value) -> Result<RemoteManifest> {
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("remote wiki command did not return ok=true");
    }
    serde_json::from_value(value).context("Failed to decode remote wiki manifest")
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
    let mod_date = 33u16;
    let flags = 0x0800u16;

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

fn resolve_export_output(output: &Path, project: &str) -> Result<PathBuf> {
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let default_name = format!("openclaw-{project}-wiki-{timestamp}.zip");
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

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

async fn collect_manifest(
    instance: &str,
    agent: &str,
    project: &str,
    include_content: bool,
    include_index: bool,
) -> Result<(RemoteManifest, Value)> {
    let (ip, key, provider) = find_deploy_record(instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let cmd = build_collect_cmd(agent, project, include_content, include_index)?;
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let value = remote_json_value(&output, "wiki manifest")?;
    handle_remote_status(&value, false)?;
    let manifest = manifest_from_value(value.clone())?;
    Ok((manifest, value))
}

pub async fn tree(params: WikiTreeParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let agent = clean_agent_id(&params.agent)?;
    let project = clean_project_slug(&params.project)?;
    let (manifest, value) = collect_manifest(&instance, &agent, &project, false, false).await?;

    if params.json {
        return print_json(&value);
    }

    println!("Workspace: {}", manifest.workspace);
    println!("Project:   {}", manifest.project);
    println!("Files:     {}", manifest.files.len());
    for file in manifest.files {
        println!(
            "  {}  {}  {}  {}",
            file.sha256,
            format_size(file.size),
            file.mtime,
            file.path
        );
    }
    Ok(())
}

pub async fn index(params: WikiIndexParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let agent = clean_agent_id(&params.agent)?;
    let project = clean_project_slug(&params.project)?;
    let (manifest, value) = collect_manifest(&instance, &agent, &project, false, true).await?;

    if params.json {
        return print_json(&value);
    }

    println!("Workspace: {}", manifest.workspace);
    println!("Project:   {}", manifest.project);
    println!("Files:     {}", manifest.files.len());
    println!("Use --json to return headings, links, tags, and hashes.");
    for file in manifest.files {
        println!("  {}  {}", file.sha256, file.path);
    }
    Ok(())
}

pub async fn read(params: WikiReadParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let agent = clean_agent_id(&params.agent)?;
    let rel_path = clean_relative_markdown_path(&params.path)?;
    let (ip, key, provider) = find_deploy_record(&instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let cmd = build_read_cmd(&agent, &rel_path)?;
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let value = remote_json_value(&output, "wiki read")?;
    handle_remote_status(&value, params.json)?;

    if params.json {
        return print_json(&value);
    }

    println!(
        "Path:   {}",
        value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(&rel_path)
    );
    println!(
        "SHA256: {}",
        value.get("sha256").and_then(Value::as_str).unwrap_or("")
    );
    println!(
        "MTime:  {}",
        value.get("mtime").and_then(Value::as_str).unwrap_or("")
    );
    println!();
    print!(
        "{}",
        value.get("content").and_then(Value::as_str).unwrap_or("")
    );
    Ok(())
}

pub async fn write(params: WikiWriteParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let agent = clean_agent_id(&params.agent)?;
    let rel_path = clean_relative_markdown_path(&params.path)?;
    let content_file = clean_content_file(params.content_file)?;
    let base_sha = clean_base_sha(&params.base_sha)?;

    let content = std::fs::read(&content_file)?;
    let remote_tmp = format!("/tmp/clawmacdo-wiki-write-{}.md", uuid::Uuid::new_v4());
    let (ip, key, provider) = find_deploy_record(&instance)?;
    let ssh_user = ssh_user_for_provider(&provider);

    let scp_ip = ip.clone();
    let scp_key = key.clone();
    let scp_user = ssh_user.to_string();
    let scp_remote_tmp = remote_tmp.clone();
    tokio::task::spawn_blocking(move || {
        clawmacdo_ssh::scp_upload_bytes(
            &scp_ip,
            &scp_key,
            &content,
            &scp_remote_tmp,
            0o644,
            &scp_user,
        )
    })
    .await
    .context("wiki content upload task failed")??;

    let cmd = build_write_cmd(&agent, &rel_path, &base_sha, &remote_tmp)?;
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let value = remote_json_value(&output, "wiki write")?;
    handle_remote_status(&value, params.json)?;

    if params.json {
        return print_json(&value);
    }

    println!(
        "Wrote {} ({}, sha256 {})",
        value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(&rel_path),
        format_size(value.get("size").and_then(Value::as_u64).unwrap_or(0)),
        value.get("sha256").and_then(Value::as_str).unwrap_or("")
    );
    Ok(())
}

pub async fn export(params: WikiExportParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let agent = clean_agent_id(&params.agent)?;
    let project = clean_project_slug(&params.project)?;
    let (manifest, _value) = collect_manifest(&instance, &agent, &project, true, false).await?;

    if manifest.files.is_empty() {
        bail!(
            "No Markdown wiki files were found for project '{}' in workspace {}.",
            manifest.project,
            manifest.workspace
        );
    }

    let mut entries = Vec::new();
    let mut total_size = 0u64;
    for file in &manifest.files {
        let data =
            BASE64
                .decode(file.data.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Remote export omitted data for {}", file.path)
                })?)
                .with_context(|| format!("Failed to decode {}", file.path))?;
        if data.len() as u64 != file.size {
            bail!(
                "Remote size mismatch for {}: expected {}, decoded {}",
                file.path,
                file.size,
                data.len()
            );
        }
        total_size += file.size;
        entries.push(ZipEntry {
            name: file.path.clone(),
            crc32: crc32(&data),
            data,
        });
    }

    let zip_path = resolve_export_output(&params.output, &project)?;
    write_zip(&zip_path, &entries)?;

    if params.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "workspace": manifest.workspace,
                "project": manifest.project,
                "output": zip_path,
                "file_count": entries.len(),
                "size": total_size
            }))?
        );
        return Ok(());
    }

    println!("Workspace: {}", manifest.workspace);
    println!("Project:   {}", manifest.project);
    println!("Files:     {}", entries.len());
    println!("Size:      {}", format_size(total_size));
    println!();
    println!("Wiki ZIP exported to: {}", zip_path.display());
    Ok(())
}

pub async fn delete(params: WikiDeleteParams) -> Result<()> {
    let instance = clean_instance(&params.instance)?;
    let agent = clean_agent_id(&params.agent)?;
    let project = clean_deletable_wiki_project_slug(&params.project)?;
    let (ip, key, provider) = find_deploy_record(&instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let cmd = build_delete_cmd(&agent, &project)?;
    let output = ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?;
    let value = remote_json_value(&output, "wiki delete")?;
    handle_remote_status(&value, params.json)?;

    if params.json {
        return print_json(&value);
    }

    let path = value.get("path").and_then(Value::as_str).unwrap_or("");
    let deleted = value
        .get("deleted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if deleted {
        println!("Deleted wiki project '{}': {}", project, path);
    } else {
        println!("Wiki project '{}' did not exist: {}", project, path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_relative_markdown_path_rejects_unsafe_paths() {
        for path in [
            "",
            "/tmp/a.md",
            "../a.md",
            "wiki/../a.md",
            "wiki//a.md",
            "wiki/a.txt",
            "wiki\\a.md",
        ] {
            assert!(clean_relative_markdown_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn clean_relative_markdown_path_accepts_workspace_markdown() {
        assert_eq!(
            clean_relative_markdown_path("llm_wiki/INDEX.md").unwrap(),
            "llm_wiki/INDEX.md"
        );
        assert_eq!(
            clean_relative_markdown_path("llm_wiki.md").unwrap(),
            "llm_wiki.md"
        );
    }

    #[test]
    fn clean_base_sha_accepts_new_or_sha256() {
        assert_eq!(clean_base_sha("NEW").unwrap(), "NEW");
        assert_eq!(clean_base_sha("new").unwrap(), "NEW");
        assert_eq!(
            clean_base_sha("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .unwrap(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert!(clean_base_sha("abc").is_err());
    }

    #[test]
    fn clean_deletable_wiki_project_slug_requires_wiki_prefix() {
        assert_eq!(
            clean_deletable_wiki_project_slug("wiki-163327").unwrap(),
            "wiki-163327"
        );
        for project in ["", "llm_wiki", "wiki-", "wiki-..", "../wiki-1", "/"] {
            assert!(
                clean_deletable_wiki_project_slug(project).is_err(),
                "{project}"
            );
        }
    }

    #[test]
    fn build_delete_cmd_targets_only_project_directory() {
        let cmd = build_delete_cmd("main", "wiki-163327").unwrap();
        assert!(cmd.contains("fs.rmSync(targetReal"));
        assert!(cmd.contains("path.dirname(target) !== workspaceReal"));
        assert!(cmd.contains("wiki-delete only accepts direct project slugs"));
        assert!(cmd.contains("wiki-163327"));
    }
}
