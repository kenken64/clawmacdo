use anyhow::{bail, Context, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct OpenclawLlmWikiParams {
    pub instance: String,
    pub agent: String,
    pub project: String,
    pub title: String,
    pub prompt: Option<String>,
    pub timeout: u64,
    pub llm_wiki_md: Option<PathBuf>,
    pub skip_claude: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenclawLlmWikiOutput {
    pub ok: bool,
    pub project: String,
    pub workspace: String,
    pub project_dir: String,
    pub llm_wiki_md: String,
    pub files: Vec<String>,
    pub uploaded_llm_wiki_md: bool,
    pub claude_status: String,
    pub claude_log: Option<String>,
    pub error: Option<String>,
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

fn clean_required(flag: &str, value: &str, max_len: usize) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("--{flag} cannot be empty.");
    }
    if trimmed.len() > max_len {
        bail!("--{flag} must be {max_len} bytes or fewer.");
    }
    if trimmed.chars().any(char::is_control) {
        bail!("--{flag} cannot contain control characters or newlines.");
    }
    Ok(trimmed.to_string())
}

fn clean_optional_prompt(value: Option<String>) -> Result<Option<String>> {
    match value {
        Some(v) if v.trim().is_empty() => Ok(None),
        Some(v) => {
            if v.len() > 4000 {
                bail!("--prompt must be 4000 bytes or fewer.");
            }
            if v.chars().any(|c| c == '\0') {
                bail!("--prompt cannot contain NUL bytes.");
            }
            Ok(Some(v.trim().to_string()))
        }
        None => Ok(None),
    }
}

fn clean_agent_id(value: &str) -> Result<String> {
    let agent = clean_required("agent", value, 80)?;
    if !agent
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        bail!("--agent may only contain letters, numbers, dots, underscores, and hyphens.");
    }
    Ok(agent)
}

fn clean_project_slug(value: &str) -> Result<String> {
    let project = clean_required("project", value, 80)?;
    if project.starts_with('.') || project.contains("..") {
        bail!("--project must be a single safe folder name, not a path.");
    }
    if !project
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        bail!("--project may only contain letters, numbers, dots, underscores, and hyphens.");
    }
    Ok(project)
}

fn clean_timeout(value: u64) -> Result<u64> {
    if !(30..=1800).contains(&value) {
        bail!("--timeout must be between 30 and 1800 seconds.");
    }
    Ok(value)
}

fn clean_llm_wiki_md(value: Option<PathBuf>) -> Result<Option<PathBuf>> {
    let Some(path) = value else {
        return Ok(None);
    };
    if !path.exists() {
        bail!("--llm-wiki-md file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("--llm-wiki-md must point to a file: {}", path.display());
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("--llm-wiki-md filename must be valid UTF-8."))?;
    let lower = file_name.to_ascii_lowercase();
    if lower != "llm_wiki.md" && !lower.ends_with(".md") {
        bail!("--llm-wiki-md must be a Markdown file.");
    }

    let metadata = std::fs::metadata(&path)?;
    if metadata.len() > 5 * 1024 * 1024 {
        bail!("--llm-wiki-md must be 5 MiB or smaller.");
    }

    Ok(Some(path.canonicalize().with_context(|| {
        format!("Failed to resolve --llm-wiki-md path {}", path.display())
    })?))
}

fn js_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn build_llm_wiki_cmd(
    params: &OpenclawLlmWikiParams,
    uploaded_llm_wiki_tmp: Option<&str>,
    json_output: bool,
) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let agent = js_string(&params.agent)?;
    let project = js_string(&params.project)?;
    let title = js_string(&params.title)?;
    let prompt = js_string(params.prompt.as_deref().unwrap_or(""))?;
    let timeout = params.timeout.to_string();
    let uploaded_llm_wiki_tmp = js_string(uploaded_llm_wiki_tmp.unwrap_or(""))?;
    let skip_claude = if params.skip_claude { "true" } else { "false" };
    let json_output = if json_output { "true" } else { "false" };

    let template = r#"set -e
export HOME="__HOME__"
export PATH="__HOME__/.local/bin:__HOME__/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"
export PROMPT_FILE="/tmp/clawmacdo-llm-wiki-prompt.txt"
export WORKSPACE_FILE="/tmp/clawmacdo-llm-wiki-workspace"
export PROJECT_DIR_FILE="/tmp/clawmacdo-llm-wiki-project-dir"
export LOG_FILE="/tmp/clawmacdo-llm-wiki-claude.log"
export SUMMARY_FILE="/tmp/clawmacdo-llm-wiki-summary.json"
export SKIP_CLAUDE="__SKIP_CLAUDE__"
export JSON_OUTPUT="__JSON_OUTPUT__"

node <<'NODE'
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '__HOME__';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = __AGENT_JSON__;
const project = __PROJECT_JSON__;
const title = __TITLE_JSON__;
const extraPrompt = __PROMPT_JSON__;
const uploadedLlmWikiTmp = __UPLOADED_LLM_WIKI_TMP_JSON__;
const promptFile = process.env.PROMPT_FILE;
const workspaceFile = process.env.WORKSPACE_FILE;
const projectDirFile = process.env.PROJECT_DIR_FILE;
const summaryFile = process.env.SUMMARY_FILE;
const jsonOutput = process.env.JSON_OUTPUT === 'true';

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

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function upsertManagedBlock(file, start, end, block) {
  let existing = '';
  try {
    existing = fs.readFileSync(file, 'utf8');
  } catch (_) {}

  const re = new RegExp(escapeRegExp(start) + '[\\s\\S]*?' + escapeRegExp(end) + '\\n?', 'm');
  const body = start + '\n' + block.trimEnd() + '\n' + end + '\n';
  let next;
  if (re.test(existing)) {
    next = existing.replace(re, body);
  } else {
    const trimmed = existing.trimEnd();
    next = body + (trimmed ? '\n' + trimmed + '\n' : '');
  }
  fs.writeFileSync(file, next, { mode: 0o644 });
}

function ensureFile(file, body) {
  if (fs.existsSync(file)) return false;
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, body.trimEnd() + '\n', { mode: 0o644 });
  return true;
}

function validateProjectSlug(value) {
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$/.test(value) || value.includes('..')) {
    throw new Error(`Invalid project slug: ${value}`);
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

validateProjectSlug(project);
const projectDir = path.normalize(path.join(workspace, project));
const relProject = path.relative(workspace, projectDir);
if (!relProject || relProject.startsWith('..') || path.isAbsolute(relProject)) {
  throw new Error(`Resolved project directory escapes workspace: ${projectDir}`);
}

fs.mkdirSync(workspace, { recursive: true });
fs.mkdirSync(projectDir, { recursive: true });

const rootFile = path.join(projectDir, 'llm_wiki.md');
if (uploadedLlmWikiTmp) {
  if (!fs.existsSync(uploadedLlmWikiTmp)) {
    throw new Error(`Uploaded llm_wiki.md was not found at ${uploadedLlmWikiTmp}`);
  }
  fs.copyFileSync(uploadedLlmWikiTmp, rootFile);
  fs.chmodSync(rootFile, 0o644);
} else {
  upsertManagedBlock(
    rootFile,
    '<!-- clawmacdo:llm-wiki:start -->',
    '<!-- clawmacdo:llm-wiki:end -->',
    [
      `# ${title}`,
      '',
      'This file is the attachable OpenClaw web-app entry point for this project LLM wiki.',
      '',
      '- Project index: [INDEX.md](INDEX.md)',
      '- Topics: [topics/](topics/)',
      '- Sources: [sources/](sources/)',
      '- Prompts: [prompts/](prompts/)',
      '- Runs: [runs/](runs/)',
      '- Decisions: [decisions/](decisions/)',
      '',
      'Keep durable project knowledge in this file or under this project folder so it can be attached as chat context.'
    ].join('\n')
  );
}

const seeded = [];
function seed(relativePath, body) {
  if (ensureFile(path.join(projectDir, relativePath), body)) seeded.push(relativePath);
}

seed('README.md', `
# ${title}

This directory holds the durable LLM wiki. Keep \`llm_wiki.md\` as the web-app attachment summary and use the rest of this folder for detailed notes.

## Layout

- \`INDEX.md\` - navigation and current map
- \`topics/\` - durable concept pages
- \`sources/\` - source notes and links
- \`prompts/\` - reusable prompt patterns
- \`runs/\` - dated work logs
- \`decisions/\` - project decisions and rationale
`);

seed('INDEX.md', `
# ${title} Index

## Start Here

- [Overview](topics/overview.md)
- [Glossary](glossary.md)
- [Open Questions](open_questions.md)

## Maintenance

- Add stable concepts to \`topics/\`.
- Add source-backed notes to \`sources/\`.
- Add dated execution notes to \`runs/\`.
- Add decisions with context to \`decisions/\`.
`);

seed('topics/overview.md', `
# Overview

Use this page for the durable summary of the project, its goals, current architecture, and important constraints.
`);

seed('sources/README.md', `
# Sources

Capture source links, excerpts, citations, and provenance notes here. Keep source notes factual and separate from conclusions.
`);

seed('prompts/README.md', `
# Prompts

Store reusable prompts and prompt patterns here.
`);

seed('runs/README.md', `
# Runs

Record dated work sessions here. Suggested filename: \`YYYY-MM-DD-topic.md\`.
`);

seed('decisions/README.md', `
# Decisions

Record project decisions, tradeoffs, date, and rationale here.
`);

seed('glossary.md', `
# Glossary

Add project terms and definitions here.
`);

seed('open_questions.md', `
# Open Questions

- What should be clarified next?
`);

const claudePrompt = [
  `You are Claude Code running on an OpenClaw instance inside this project directory: ${projectDir}`,
  '',
  `Create or refine the LLM wiki project structure for "${title}" in project "${project}".`,
  '',
  'Requirements:',
  '- Work inside this project directory only.',
  '- Keep `llm_wiki.md` in the project root. It is the attachable web-app summary and entry point.',
  '- Keep detailed wiki files in this project folder and its subdirectories.',
  '- Preserve existing content outside this project directory.',
  '- Do not delete or rename unrelated workspace files.',
  '- Use concise Markdown, relative links, and clear headings.',
  '- Update `INDEX.md` so a reader can navigate the wiki.',
  '- Add useful starter pages only when they clarify the structure.',
  extraPrompt ? `Additional instructions:\n${extraPrompt}` : ''
].filter(Boolean).join('\n');

fs.writeFileSync(promptFile, claudePrompt, { mode: 0o600 });
fs.writeFileSync(workspaceFile, workspace, { mode: 0o600 });
fs.writeFileSync(projectDirFile, projectDir, { mode: 0o600 });

const files = [
  'llm_wiki.md',
  'README.md',
  'INDEX.md',
  'topics/overview.md',
  'sources/README.md',
  'prompts/README.md',
  'runs/README.md',
  'decisions/README.md',
  'glossary.md',
  'open_questions.md'
];
const summary = {
  ok: true,
  project,
  workspace,
  project_dir: projectDir,
  llm_wiki_md: rootFile,
  files,
  uploaded_llm_wiki_md: Boolean(uploadedLlmWikiTmp),
  claude_status: process.env.SKIP_CLAUDE === 'true' ? 'skipped' : 'pending',
  claude_log: null,
  error: null
};
fs.writeFileSync(summaryFile, JSON.stringify(summary, null, 2) + '\n', { mode: 0o600 });

if (!jsonOutput) {
  console.log(`workspace=${workspace}`);
  console.log(`project=${project}`);
  console.log(`project_dir=${projectDir}`);
  console.log(`llm_wiki_md=${rootFile}`);
  console.log(`uploaded_llm_wiki_md=${uploadedLlmWikiTmp ? 'yes' : 'no'}`);
  console.log(`seeded_files=${seeded.length ? seeded.join(',') : 'none'}`);
}
NODE

WORKSPACE="$(cat "$WORKSPACE_FILE")"
PROJECT_DIR="$(cat "$PROJECT_DIR_FILE")"

update_summary() {
  OK_VALUE="$1" CLAUDE_STATUS_VALUE="$2" ERROR_VALUE="${3:-}" CLAUDE_LOG_VALUE="${4:-}" node <<'NODE'
const fs = require('fs');
const file = process.env.SUMMARY_FILE;
const summary = JSON.parse(fs.readFileSync(file, 'utf8'));
summary.ok = process.env.OK_VALUE === 'true';
summary.claude_status = process.env.CLAUDE_STATUS_VALUE || summary.claude_status;
summary.error = process.env.ERROR_VALUE || null;
summary.claude_log = process.env.CLAUDE_LOG_VALUE || null;
fs.writeFileSync(file, JSON.stringify(summary, null, 2) + '\n', { mode: 0o600 });
NODE
}

emit_summary() {
  cat "$SUMMARY_FILE"
}

finish_success() {
  update_summary true "$1" "" "${2:-}"
  if [ "$JSON_OUTPUT" = "true" ]; then
    emit_summary
  fi
  exit 0
}

finish_failure() {
  EXIT_STATUS="$1"
  CLAUDE_STATUS="$2"
  ERROR_TEXT="$3"
  LOG_PATH="${4:-}"
  update_summary false "$CLAUDE_STATUS" "$ERROR_TEXT" "$LOG_PATH"
  if [ "$JSON_OUTPUT" = "true" ]; then
    emit_summary
    exit 0
  fi
  echo "claude=$CLAUDE_STATUS"
  echo "claude_status=$EXIT_STATUS"
  [ -n "$LOG_PATH" ] && echo "claude_log=$LOG_PATH"
  echo "error:"
  printf '%s\n' "$ERROR_TEXT"
  exit "$EXIT_STATUS"
}

if [ "$SKIP_CLAUDE" = "true" ]; then
  if [ ! -f "$PROJECT_DIR/llm_wiki.md" ]; then
    finish_failure 1 failed "Seeded project is missing $PROJECT_DIR/llm_wiki.md." ""
  fi
  if [ "$JSON_OUTPUT" != "true" ]; then
    echo "claude=skipped"
    echo "ready=$PROJECT_DIR/llm_wiki.md"
  fi
  finish_success skipped ""
fi

if ! command -v claude >/dev/null 2>&1; then
  finish_failure 127 missing "Claude Code binary not found in PATH. Install or repair Claude Code on the instance before rerunning this command." ""
fi

set +e
CLAUDE_VERSION_RAW="$(claude --version 2>&1)"
CLAUDE_VERSION_STATUS=$?
set -e
if [ "$CLAUDE_VERSION_STATUS" -ne 0 ]; then
  INSTALL_CJS="$(find "$HOME/.local/share/pnpm" "$HOME/.npm" -path '*/@anthropic-ai/claude-code/install.cjs' -type f 2>/dev/null | head -1 || true)"
  if [ -n "$INSTALL_CJS" ]; then
    node "$INSTALL_CJS" >/tmp/clawmacdo-llm-wiki-claude-repair.log 2>&1 || true
  fi
fi

set +e
CLAUDE_VERSION_RAW="$(claude --version 2>&1)"
CLAUDE_VERSION_STATUS=$?
set -e
if [ "$CLAUDE_VERSION_STATUS" -ne 0 ]; then
  REPAIR_TAIL="$(tail -40 /tmp/clawmacdo-llm-wiki-claude-repair.log 2>/dev/null || true)"
  ERROR_TEXT="$(printf 'Claude Code is installed but unavailable after repair attempt.\n\nclaude --version output:\n%s\n\nrepair log:\n%s' "$CLAUDE_VERSION_RAW" "$REPAIR_TAIL")"
  finish_failure 127 unavailable "$ERROR_TEXT" "/tmp/clawmacdo-llm-wiki-claude-repair.log"
fi
CLAUDE_VERSION="$(printf '%s\n' "$CLAUDE_VERSION_RAW" | head -1)"

cd "$PROJECT_DIR"
if [ "$JSON_OUTPUT" != "true" ]; then
  echo "claude=$CLAUDE_VERSION"
fi
set +e
if command -v timeout >/dev/null 2>&1; then
  timeout __TIMEOUT__s claude -p "$(cat "$PROMPT_FILE")" --dangerously-skip-permissions --output-format text >"$LOG_FILE" 2>&1
else
  claude -p "$(cat "$PROMPT_FILE")" --dangerously-skip-permissions --output-format text >"$LOG_FILE" 2>&1
fi
STATUS=$?
set -e

if [ "$JSON_OUTPUT" != "true" ]; then
  echo "claude_status=$STATUS"
  echo "claude_log=$LOG_FILE"
fi
if [ "$STATUS" -ne 0 ]; then
  LOG_TAIL="$(tail -40 "$LOG_FILE" 2>/dev/null || true)"
  if [ "$STATUS" -eq 124 ]; then
    ERROR_TEXT="$(printf 'Claude Code timed out after __TIMEOUT__ seconds.\n\n%s' "$LOG_TAIL")"
  else
    ERROR_TEXT="$(printf 'Claude Code exited with status %s.\n\n%s' "$STATUS" "$LOG_TAIL")"
  fi
  finish_failure "$STATUS" failed "$ERROR_TEXT" "$LOG_FILE"
fi

if [ ! -f "$PROJECT_DIR/llm_wiki.md" ]; then
  finish_failure 1 failed "Claude Code completed but $PROJECT_DIR/llm_wiki.md is missing." "$LOG_FILE"
fi

if [ "$JSON_OUTPUT" != "true" ]; then
  echo "ready=$PROJECT_DIR/llm_wiki.md"
fi
finish_success success "$LOG_FILE"
"#;

    Ok(template
        .replace("__HOME__", home)
        .replace("__AGENT_JSON__", &agent)
        .replace("__PROJECT_JSON__", &project)
        .replace("__TITLE_JSON__", &title)
        .replace("__PROMPT_JSON__", &prompt)
        .replace("__UPLOADED_LLM_WIKI_TMP_JSON__", &uploaded_llm_wiki_tmp)
        .replace("__SKIP_CLAUDE__", skip_claude)
        .replace("__JSON_OUTPUT__", json_output)
        .replace("__TIMEOUT__", &timeout))
}

fn clean_params(params: OpenclawLlmWikiParams) -> Result<OpenclawLlmWikiParams> {
    Ok(OpenclawLlmWikiParams {
        instance: clean_required("instance", &params.instance, 255)?,
        agent: clean_agent_id(&params.agent)?,
        project: clean_project_slug(&params.project)?,
        title: clean_required("title", &params.title, 160)?,
        prompt: clean_optional_prompt(params.prompt)?,
        timeout: clean_timeout(params.timeout)?,
        llm_wiki_md: clean_llm_wiki_md(params.llm_wiki_md)?,
        skip_claude: params.skip_claude,
        json: params.json,
    })
}

async fn execute_remote(
    params: &OpenclawLlmWikiParams,
    json_output: bool,
) -> Result<(String, String)> {
    let (ip, key, provider) = find_deploy_record(&params.instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let uploaded_llm_wiki = params.llm_wiki_md.clone();

    let remote_tmp = uploaded_llm_wiki
        .as_ref()
        .map(|_| format!("/tmp/clawmacdo-llm-wiki-{}.md", uuid::Uuid::new_v4()));
    let cmd = build_llm_wiki_cmd(params, remote_tmp.as_deref(), json_output)?;

    let output = if let (Some(path), Some(remote_tmp)) = (uploaded_llm_wiki, remote_tmp.as_deref())
    {
        let bytes = std::fs::read(&path)?;
        let scp_ip = ip.clone();
        let scp_key = key.clone();
        let scp_user = ssh_user.to_string();
        let cmd_owned = cmd;
        let remote_tmp_owned = remote_tmp.to_string();
        let outputs = tokio::task::spawn_blocking(move || {
            let cmds: Vec<&str> = vec![&cmd_owned];
            clawmacdo_ssh::scp_upload_bytes_and_exec_as(
                &scp_ip,
                &scp_key,
                &bytes,
                &remote_tmp_owned,
                0o644,
                &cmds,
                &scp_user,
            )
        })
        .await??;
        outputs.first().cloned().unwrap_or_default()
    } else {
        ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await?
    };

    Ok((ip, output))
}

fn parse_json_output(output: &str) -> Result<OpenclawLlmWikiOutput> {
    let trimmed = output.trim();
    let start = trimmed
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("No JSON object found in openclaw-llm-wiki output."))?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| anyhow::anyhow!("Incomplete JSON object in openclaw-llm-wiki output."))?;
    serde_json::from_str(&trimmed[start..=end])
        .with_context(|| format!("Failed to parse openclaw-llm-wiki JSON output: {}", trimmed))
}

pub async fn run_for_result(params: OpenclawLlmWikiParams) -> Result<OpenclawLlmWikiOutput> {
    let params = clean_params(params)?;
    let (_, output) = execute_remote(&params, true).await?;
    parse_json_output(&output)
}

pub async fn run(params: OpenclawLlmWikiParams) -> Result<()> {
    let params = clean_params(params)?;

    if !params.json {
        println!("Creating OpenClaw LLM wiki on {}...", params.instance);
        if let Some(path) = &params.llm_wiki_md {
            println!(
                "[1/3] Uploading {} as {}/llm_wiki.md...",
                path.display(),
                params.project
            );
        }
        println!(
            "[2/3] Seeding {}/llm_wiki.md and project wiki files...",
            params.project
        );
        if params.skip_claude {
            println!("[3/3] Skipping Claude Code as requested...");
        } else {
            println!("[3/3] Launching Claude Code to refine the project wiki...");
        }
    }

    let (ip, output) = execute_remote(&params, params.json).await?;

    if params.json {
        let parsed = parse_json_output(&output)?;
        println!("{}", serde_json::to_string_pretty(&parsed)?);
        return Ok(());
    }

    for line in output.trim().lines() {
        println!("  {line}");
    }

    println!();
    println!("OpenClaw LLM wiki initialized on {ip}:");
    println!("  Agent: {}", params.agent);
    println!("  Project: {}", params.project);
    println!("  Attach: {}/llm_wiki.md", params.project);
    println!("  Details: {}/", params.project);
    if params.skip_claude {
        println!("  Claude: skipped");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_agent_id_rejects_path_segments() {
        assert!(clean_agent_id("../main").is_err());
        assert_eq!(clean_agent_id("main.agent_1").unwrap(), "main.agent_1");
    }

    #[test]
    fn clean_project_slug_rejects_paths() {
        assert!(clean_project_slug("../wiki").is_err());
        assert!(clean_project_slug(".hidden").is_err());
        assert!(clean_project_slug("wiki/name").is_err());
        assert_eq!(
            clean_project_slug("project-wiki_1").unwrap(),
            "project-wiki_1"
        );
    }

    #[test]
    fn clean_timeout_enforces_bounds() {
        assert!(clean_timeout(29).is_err());
        assert!(clean_timeout(1801).is_err());
        assert_eq!(clean_timeout(600).unwrap(), 600);
    }

    #[test]
    fn build_command_targets_attachable_root_file() {
        let cmd = build_llm_wiki_cmd(
            &OpenclawLlmWikiParams {
                instance: "example".to_string(),
                agent: "main".to_string(),
                project: "my-project".to_string(),
                title: "LLM Wiki".to_string(),
                prompt: Some("Add project constraints.".to_string()),
                timeout: 600,
                llm_wiki_md: None,
                skip_claude: false,
                json: false,
            },
            None,
            false,
        )
        .unwrap();

        assert!(cmd.contains("llm_wiki.md"));
        assert!(cmd.contains("my-project"));
        assert!(cmd.contains("INDEX.md"));
        assert!(cmd.contains("claude -p"));
        assert!(cmd.contains("Add project constraints."));
    }

    #[test]
    fn build_command_can_skip_claude_after_upload() {
        let cmd = build_llm_wiki_cmd(
            &OpenclawLlmWikiParams {
                instance: "example".to_string(),
                agent: "main".to_string(),
                project: "llm_wiki".to_string(),
                title: "LLM Wiki".to_string(),
                prompt: None,
                timeout: 600,
                llm_wiki_md: None,
                skip_claude: true,
                json: false,
            },
            Some("/tmp/uploaded-llm-wiki.md"),
            false,
        )
        .unwrap();

        assert!(cmd.contains("llm_wiki.md"));
        assert!(cmd.contains("uploaded_llm_wiki_md="));
        assert!(cmd.contains("claude=skipped"));
        assert!(cmd.contains("/tmp/uploaded-llm-wiki.md"));
    }

    #[test]
    fn build_command_json_mode_has_summary_contract() {
        let cmd = build_llm_wiki_cmd(
            &OpenclawLlmWikiParams {
                instance: "example".to_string(),
                agent: "main".to_string(),
                project: "docs".to_string(),
                title: "Docs".to_string(),
                prompt: None,
                timeout: 600,
                llm_wiki_md: None,
                skip_claude: false,
                json: true,
            },
            None,
            true,
        )
        .unwrap();

        assert!(cmd.contains("project,"));
        assert!(cmd.contains("files,"));
        assert!(cmd.contains("claude_status:"));
        assert!(cmd.contains("emit_summary"));
    }
}
