use anyhow::{bail, Context, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use std::path::PathBuf;

pub struct OpenclawLlmWikiParams {
    pub instance: String,
    pub agent: String,
    pub title: String,
    pub prompt: Option<String>,
    pub timeout: u64,
    pub llm_wiki_md: Option<PathBuf>,
    pub skip_claude: bool,
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
) -> Result<String> {
    let home = config::OPENCLAW_HOME;
    let agent = js_string(&params.agent)?;
    let title = js_string(&params.title)?;
    let prompt = js_string(params.prompt.as_deref().unwrap_or(""))?;
    let timeout = params.timeout.to_string();
    let uploaded_llm_wiki_tmp = js_string(uploaded_llm_wiki_tmp.unwrap_or(""))?;
    let skip_claude = if params.skip_claude { "true" } else { "false" };

    let template = r#"set -e
export HOME="__HOME__"
export PATH="__HOME__/.local/bin:__HOME__/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"
export PROMPT_FILE="/tmp/clawmacdo-llm-wiki-prompt.txt"
export WORKSPACE_FILE="/tmp/clawmacdo-llm-wiki-workspace"
export LOG_FILE="/tmp/clawmacdo-llm-wiki-claude.log"
export SKIP_CLAUDE="__SKIP_CLAUDE__"

node <<'NODE'
const fs = require('fs');
const path = require('path');

const home = process.env.HOME || '__HOME__';
const configPath = path.join(home, '.openclaw', 'openclaw.json');
const agentId = __AGENT_JSON__;
const title = __TITLE_JSON__;
const extraPrompt = __PROMPT_JSON__;
const uploadedLlmWikiTmp = __UPLOADED_LLM_WIKI_TMP_JSON__;
const promptFile = process.env.PROMPT_FILE;
const workspaceFile = process.env.WORKSPACE_FILE;

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

const cfg = readJson(configPath);
const agents = cfg.agents || {};
const list = Array.isArray(agents.list) ? agents.list : [];
const agent = list.find((item) => item && item.id === agentId);
const workspace = expandWorkspace(
  agent && agent.workspace
    ? agent.workspace
    : agents.defaults && agents.defaults.workspace
);

const wikiDir = path.join(workspace, 'llm_wiki');
fs.mkdirSync(workspace, { recursive: true });
fs.mkdirSync(wikiDir, { recursive: true });

const rootFile = path.join(workspace, 'llm_wiki.md');
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
      'This file is the attachable OpenClaw web-app entry point for the local LLM wiki.',
      '',
      '- Project index: [llm_wiki/INDEX.md](llm_wiki/INDEX.md)',
      '- Topics: [llm_wiki/topics/](llm_wiki/topics/)',
      '- Sources: [llm_wiki/sources/](llm_wiki/sources/)',
      '- Prompts: [llm_wiki/prompts/](llm_wiki/prompts/)',
      '- Runs: [llm_wiki/runs/](llm_wiki/runs/)',
      '- Decisions: [llm_wiki/decisions/](llm_wiki/decisions/)',
      '',
      'Keep durable project knowledge in this file or under `llm_wiki/` so it can be attached as chat context.'
    ].join('\n')
  );
}

const seeded = [];
function seed(relativePath, body) {
  if (ensureFile(path.join(wikiDir, relativePath), body)) seeded.push(relativePath);
}

seed('README.md', `
# ${title}

This directory holds the durable LLM wiki. Keep the root \`../llm_wiki.md\` as the web-app attachment summary and use this folder for detailed notes.

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
  `You are Claude Code running on an OpenClaw instance inside this workspace: ${workspace}`,
  '',
  `Create or refine the LLM wiki project structure for "${title}".`,
  '',
  'Requirements:',
  '- Keep `llm_wiki.md` at the workspace root. It is the attachable web-app summary and entry point.',
  '- Keep detailed wiki files under `llm_wiki/`.',
  '- Preserve existing content outside `llm_wiki.md` and `llm_wiki/`.',
  '- Do not delete or rename unrelated workspace files.',
  '- Use concise Markdown, relative links, and clear headings.',
  '- Update `llm_wiki/INDEX.md` so a reader can navigate the wiki.',
  '- Add useful starter pages only when they clarify the structure.',
  extraPrompt ? `Additional instructions:\n${extraPrompt}` : ''
].filter(Boolean).join('\n');

fs.writeFileSync(promptFile, claudePrompt, { mode: 0o600 });
fs.writeFileSync(workspaceFile, workspace, { mode: 0o600 });

console.log(`workspace=${workspace}`);
console.log(`llm_wiki_md=${rootFile}`);
console.log(`wiki_dir=${wikiDir}`);
console.log(`uploaded_llm_wiki_md=${uploadedLlmWikiTmp ? 'yes' : 'no'}`);
console.log(`seeded_files=${seeded.length ? seeded.join(',') : 'none'}`);
NODE

WORKSPACE="$(cat "$WORKSPACE_FILE")"
if [ "$SKIP_CLAUDE" = "true" ]; then
  test -f "$WORKSPACE/llm_wiki.md"
  test -d "$WORKSPACE/llm_wiki"
  echo "claude=skipped"
  echo "ready=$WORKSPACE/llm_wiki.md"
  exit 0
fi

if ! command -v claude >/dev/null 2>&1; then
  echo "claude=missing"
  echo "Install or repair Claude Code on the instance before rerunning this command."
  exit 127
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
  echo "claude=unavailable"
  echo "$CLAUDE_VERSION_RAW"
  echo "claude_repair_log=/tmp/clawmacdo-llm-wiki-claude-repair.log"
  exit 127
fi
CLAUDE_VERSION="$(printf '%s\n' "$CLAUDE_VERSION_RAW" | head -1)"

cd "$WORKSPACE"
echo "claude=$CLAUDE_VERSION"
set +e
if command -v timeout >/dev/null 2>&1; then
  timeout __TIMEOUT__s claude -p "$(cat "$PROMPT_FILE")" --dangerously-skip-permissions --output-format text >"$LOG_FILE" 2>&1
else
  claude -p "$(cat "$PROMPT_FILE")" --dangerously-skip-permissions --output-format text >"$LOG_FILE" 2>&1
fi
STATUS=$?
set -e

echo "claude_status=$STATUS"
echo "claude_log=$LOG_FILE"
if [ "$STATUS" -ne 0 ]; then
  tail -40 "$LOG_FILE" 2>/dev/null || true
  exit "$STATUS"
fi

test -f "$WORKSPACE/llm_wiki.md"
test -d "$WORKSPACE/llm_wiki"
echo "ready=$WORKSPACE/llm_wiki.md"
"#;

    Ok(template
        .replace("__HOME__", home)
        .replace("__AGENT_JSON__", &agent)
        .replace("__TITLE_JSON__", &title)
        .replace("__PROMPT_JSON__", &prompt)
        .replace("__UPLOADED_LLM_WIKI_TMP_JSON__", &uploaded_llm_wiki_tmp)
        .replace("__SKIP_CLAUDE__", skip_claude)
        .replace("__TIMEOUT__", &timeout))
}

pub async fn run(params: OpenclawLlmWikiParams) -> Result<()> {
    let params = OpenclawLlmWikiParams {
        instance: clean_required("instance", &params.instance, 255)?,
        agent: clean_agent_id(&params.agent)?,
        title: clean_required("title", &params.title, 160)?,
        prompt: clean_optional_prompt(params.prompt)?,
        timeout: clean_timeout(params.timeout)?,
        llm_wiki_md: clean_llm_wiki_md(params.llm_wiki_md)?,
        skip_claude: params.skip_claude,
    };

    let (ip, key, provider) = find_deploy_record(&params.instance)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let uploaded_llm_wiki = params.llm_wiki_md.clone();

    println!("Creating OpenClaw LLM wiki on {ip}...");
    if let Some(path) = &uploaded_llm_wiki {
        println!("[1/3] Uploading {} as llm_wiki.md...", path.display());
    }
    println!("[2/3] Seeding llm_wiki.md and llm_wiki/ in the active workspace...");
    if params.skip_claude {
        println!("[3/3] Skipping Claude Code as requested...");
    } else {
        println!("[3/3] Launching Claude Code to refine the wiki structure...");
    }

    let remote_tmp = uploaded_llm_wiki
        .as_ref()
        .map(|_| format!("/tmp/clawmacdo-llm-wiki-{}.md", uuid::Uuid::new_v4()));
    let cmd = build_llm_wiki_cmd(&params, remote_tmp.as_deref())?;

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

    for line in output.trim().lines() {
        println!("  {line}");
    }

    println!();
    println!("OpenClaw LLM wiki initialized on {ip}:");
    println!("  Agent: {}", params.agent);
    println!("  Attach: llm_wiki.md");
    println!("  Details: llm_wiki/");
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
                title: "LLM Wiki".to_string(),
                prompt: Some("Add project constraints.".to_string()),
                timeout: 600,
                llm_wiki_md: None,
                skip_claude: false,
            },
            None,
        )
        .unwrap();

        assert!(cmd.contains("llm_wiki.md"));
        assert!(cmd.contains("llm_wiki/INDEX.md"));
        assert!(cmd.contains("claude -p"));
        assert!(cmd.contains("Add project constraints."));
    }

    #[test]
    fn build_command_can_skip_claude_after_upload() {
        let cmd = build_llm_wiki_cmd(
            &OpenclawLlmWikiParams {
                instance: "example".to_string(),
                agent: "main".to_string(),
                title: "LLM Wiki".to_string(),
                prompt: None,
                timeout: 600,
                llm_wiki_md: None,
                skip_claude: true,
            },
            Some("/tmp/uploaded-llm-wiki.md"),
        )
        .unwrap();

        assert!(cmd.contains("llm_wiki.md"));
        assert!(cmd.contains("uploaded_llm_wiki_md="));
        assert!(cmd.contains("claude=skipped"));
        assert!(cmd.contains("/tmp/uploaded-llm-wiki.md"));
    }
}
