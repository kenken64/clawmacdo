use crate::commands::deploy::{self, DeployParams};
use crate::config;
use crate::provision::commands::ssh_as_openclaw_async;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;

// ── Shared state ────────────────────────────────────────────────────────────

type Jobs = Arc<RwLock<HashMap<String, DeployJob>>>;

struct DeployJob {
    status: JobStatus,
    rx: Option<mpsc::UnboundedReceiver<String>>,
}

#[derive(Clone, Copy, PartialEq)]
enum JobStatus {
    Running,
    Completed,
    Failed,
}

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Deserialize)]
struct DeployRequest {
    do_token: String,
    anthropic_key: String,
    #[serde(default)]
    openai_key: String,
    #[serde(default)]
    gemini_key: String,
    #[serde(default)]
    whatsapp_phone_number: String,
    #[serde(default)]
    telegram_bot_token: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    backup: String,
    #[serde(default)]
    enable_backups: bool,
    #[serde(default)]
    enable_sandbox: bool,
    #[serde(default)]
    tailscale: bool,
}

#[derive(Serialize)]
struct DeployResponse {
    deploy_id: String,
}

#[derive(Serialize)]
struct BackupEntry {
    name: String,
    path: String,
    size: u64,
}

#[derive(Deserialize)]
struct TelegramPairingApproveRequest {
    ip: String,
    ssh_key_path: String,
    pairing_code: String,
}

#[derive(Serialize)]
struct TelegramPairingApproveResponse {
    ok: bool,
    message: String,
}

#[derive(Deserialize)]
struct WhatsAppQrRequest {
    ip: String,
    ssh_key_path: String,
}

#[derive(Serialize)]
struct WhatsAppQrResponse {
    ok: bool,
    message: String,
    qr_output: String,
}

// ── Entry point ─────────────────────────────────────────────────────────────

pub async fn run(port: u16) -> anyhow::Result<()> {
    let jobs: Jobs = Arc::new(RwLock::new(HashMap::new()));

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/assets/mascot.jpg", get(mascot_handler))
        .route("/api/backups", get(list_backups_handler))
        .route("/api/deploy", post(start_deploy_handler))
        .route("/api/deploy/{id}/events", get(deploy_events_handler))
        .route("/api/telegram/pairing/approve", post(approve_telegram_pairing_handler))
        .route("/api/whatsapp/qr", post(fetch_whatsapp_qr_handler))
        .with_state(jobs);

    let addr = format!("0.0.0.0:{port}");
    println!("ClawMacToDO web UI running at http://localhost:{port}");
    println!("Press Ctrl+C to stop.\n");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ── Route handlers ──────────────────────────────────────────────────────────

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn mascot_handler() -> impl IntoResponse {
    const MASCOT: &[u8] = include_bytes!("../../assets/mascot.jpg");
    (
        [("content-type", "image/jpeg"), ("cache-control", "public, max-age=86400")],
        MASCOT,
    )
}

async fn list_backups_handler() -> impl IntoResponse {
    let entries = list_backup_files().unwrap_or_default();
    Json(entries)
}

async fn start_deploy_handler(
    State(jobs): State<Jobs>,
    Json(req): Json<DeployRequest>,
) -> impl IntoResponse {
    let deploy_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::unbounded_channel::<String>();

    // Parse backup path
    let backup: Option<PathBuf> = if req.backup.is_empty() || req.backup == "none" {
        None
    } else {
        Some(PathBuf::from(&req.backup))
    };

    let job = DeployJob {
        status: JobStatus::Running,
        rx: Some(rx),
    };
    jobs.write().await.insert(deploy_id.clone(), job);

    let id = deploy_id.clone();
    let jobs_clone = jobs.clone();

    tokio::spawn(async move {
        let params = DeployParams {
            do_token: req.do_token,
            anthropic_key: req.anthropic_key,
            openai_key: req.openai_key,
            gemini_key: req.gemini_key,
            whatsapp_phone_number: req.whatsapp_phone_number,
            telegram_bot_token: req.telegram_bot_token,
            region: if req.region.is_empty() { None } else { Some(req.region) },
            size: if req.size.is_empty() { None } else { Some(req.size) },
            hostname: if req.hostname.is_empty() { None } else { Some(req.hostname) },
            backup,
            enable_backups: req.enable_backups,
            enable_sandbox: req.enable_sandbox,
            tailscale: req.tailscale,
            non_interactive: true,
            progress_tx: Some(tx.clone()),
        };

        let result = deploy::run(params).await;

        let final_status = match &result {
            Ok(record) => {
                let _ = tx.send(format!(
                    "DEPLOY_COMPLETE:{}:{}:{}",
                    record.ip_address, record.ssh_key_path, record.hostname
                ));
                JobStatus::Completed
            }
            Err(e) => {
                let _ = tx.send(format!("DEPLOY_ERROR:{e:#}"));
                JobStatus::Failed
            }
        };

        if let Some(job) = jobs_clone.write().await.get_mut(&id) {
            job.status = final_status;
        }
    });

    Json(DeployResponse { deploy_id })
}

async fn deploy_events_handler(
    State(jobs): State<Jobs>,
    Path(id): Path<String>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    // Take the receiver out of the job (only one SSE listener per deploy)
    let rx = {
        let mut map = jobs.write().await;
        map.get_mut(&id).and_then(|job| job.rx.take())
    };

    let stream = match rx {
        Some(rx) => UnboundedReceiverStream::new(rx),
        None => {
            // No job found or already consumed — return an empty stream
            let (_tx, rx) = mpsc::unbounded_channel();
            UnboundedReceiverStream::new(rx)
        }
    };

    Sse::new(stream.map(|msg| Ok(Event::default().data(msg))))
}

async fn approve_telegram_pairing_handler(
    Json(req): Json<TelegramPairingApproveRequest>,
) -> impl IntoResponse {
    let ip = req.ip.trim().to_string();
    let key_path = req.ssh_key_path.trim().to_string();
    if ip.is_empty() || key_path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(TelegramPairingApproveResponse {
                ok: false,
                message: "Missing IP or SSH key path.".into(),
            }),
        );
    }

    let code = match normalize_pairing_code(&req.pairing_code) {
        Some(code) => code,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(TelegramPairingApproveResponse {
                    ok: false,
                    message: "Invalid pairing code. Use 8 letters/numbers.".into(),
                }),
            )
        }
    };

    let key = PathBuf::from(key_path);
    let cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         openclaw pairing approve telegram {code} --notify 2>&1",
        home = config::OPENCLAW_HOME,
        code = code,
    );

    match ssh_as_openclaw_async(&ip, &key, &cmd).await {
        Ok(out) => {
            let msg = out.trim();
            (
                StatusCode::OK,
                Json(TelegramPairingApproveResponse {
                    ok: true,
                    message: if msg.is_empty() {
                        "Telegram pairing approved.".into()
                    } else {
                        msg.to_string()
                    },
                }),
            )
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(TelegramPairingApproveResponse {
                ok: false,
                message: format!("Failed to approve pairing: {e}"),
            }),
        ),
    }
}

async fn fetch_whatsapp_qr_handler(Json(req): Json<WhatsAppQrRequest>) -> impl IntoResponse {
    let ip = req.ip.trim().to_string();
    let key_path = req.ssh_key_path.trim().to_string();
    if ip.is_empty() || key_path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(WhatsAppQrResponse {
                ok: false,
                message: "Missing IP or SSH key path.".into(),
                qr_output: String::new(),
            }),
        );
    }

    let key = PathBuf::from(key_path);
    let cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if command -v timeout >/dev/null 2>&1; then \
           timeout 240s openclaw channels login --channel whatsapp 2>&1 || true; \
         else \
           openclaw channels login --channel whatsapp 2>&1; \
         fi",
        home = config::OPENCLAW_HOME,
    );

    match ssh_as_openclaw_async(&ip, &key, &cmd).await {
        Ok(out) => (
            StatusCode::OK,
            Json(WhatsAppQrResponse {
                ok: true,
                message: "WhatsApp login output captured.".into(),
                qr_output: out,
            }),
        ),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(WhatsAppQrResponse {
                ok: false,
                message: format!("Failed to fetch WhatsApp QR: {e}"),
                qr_output: String::new(),
            }),
        ),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn normalize_pairing_code(raw: &str) -> Option<String> {
    let code = raw.trim().to_ascii_uppercase();
    if code.len() == 8 && code.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(code)
    } else {
        None
    }
}

fn list_backup_files() -> anyhow::Result<Vec<BackupEntry>> {
    let backups_dir = config::backups_dir()?;
    let mut entries = Vec::new();

    if !backups_dir.exists() {
        return Ok(entries);
    }

    for entry in std::fs::read_dir(&backups_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("gz") {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            entries.push(BackupEntry {
                name,
                path: path.display().to_string(),
                size,
            });
        }
    }

    entries.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(entries)
}

// ── Embedded HTML ───────────────────────────────────────────────────────────

const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en" class="dark">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>ClawMacToDO</title>
<script src="https://cdn.tailwindcss.com"></script>
<script>
tailwind.config = {
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        slate: {
          850: '#172033',
          950: '#0a0f1a',
        }
      }
    }
  }
}
</script>
<style>
  @keyframes pulse-border { 0%,100%{border-color:rgba(59,130,246,0.5)} 50%{border-color:rgba(59,130,246,1)} }
  .pulse-border { animation: pulse-border 2s ease-in-out infinite; }
  .deploy-log-area::-webkit-scrollbar { width: 8px; }
  .deploy-log-area::-webkit-scrollbar-track { background: #0f172a; }
  .deploy-log-area::-webkit-scrollbar-thumb { background: #334155; border-radius: 4px; }
  .eye-btn { position:absolute; right:8px; top:50%; transform:translateY(-50%); cursor:pointer; color:#94a3b8; padding:4px; }
  .eye-btn:hover { color:#e2e8f0; }
  @media (min-width: 640px) { .eye-btn { right:12px; } }
  .deploy-summary-content p { word-break: break-all; }
  @media (max-width: 639px) {
    .deploy-log-area { height: 200px; font-size: 0.75rem; padding: 0.75rem; }
  }
</style>
</head>
<body class="bg-slate-950 text-slate-200 min-h-screen">

<!-- Header -->
<header class="border-b border-slate-800 bg-slate-950/80 backdrop-blur sticky top-0 z-10">
  <div class="max-w-4xl mx-auto px-4 sm:px-6 py-3 sm:py-4 flex items-center gap-2 sm:gap-3">
    <svg class="w-7 h-7 sm:w-8 sm:h-8 text-blue-400 shrink-0" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M5 12h14M12 5l7 7-7 7"/></svg>
    <h1 class="text-lg sm:text-xl font-bold tracking-tight">ClawMacToDO</h1>
    <span class="text-xs sm:text-sm text-slate-500 ml-1 sm:ml-2 hidden xs:inline">Deploy OpenClaw to DigitalOcean</span>
  </div>
</header>

<main class="max-w-4xl mx-auto px-3 sm:px-6 py-6 sm:py-8">

<!-- Mascot -->
<div class="flex justify-center mb-6">
  <img src="/assets/mascot.jpg" alt="ClawMacToDO Mascot" class="rounded-xl shadow-lg max-w-xs sm:max-w-sm w-full">
</div>

<!-- Add Deployment button -->
<button type="button" onclick="addDeployCard()" class="w-full mb-6 bg-blue-600 hover:bg-blue-500 text-white font-semibold py-2.5 sm:py-3 text-sm sm:text-base rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-blue-400 focus:ring-offset-2 focus:ring-offset-slate-900 flex items-center justify-center gap-2">
  <svg class="w-5 h-5" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15"/></svg>
  Add Deployment
</button>

<!-- Deploy cards container -->
<div id="deploys-container" class="space-y-6"></div>

</main>

<script>
const TOTAL_STEPS = 16;
let deployCounter = 0;
let backupOptions = '<option value="none">None</option>';

const eyeClosed = '<svg class="w-5 h-5 eye-closed" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"/></svg>';
const eyeOpen = '<svg class="w-5 h-5 eye-open hidden" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>';

function eyeBtn() {
  return `<button type="button" class="eye-btn" onclick="toggleEye(this)">${eyeClosed}${eyeOpen}</button>`;
}

function passwordField(name, label, placeholder, required) {
  const req = required ? '<span class="text-red-400">*</span>' : '<span class="text-slate-500">(optional)</span>';
  const reqAttr = required ? 'required' : '';
  return `<div>
    <label class="block text-sm font-medium text-slate-300 mb-1">${label} ${req}</label>
    <div class="relative">
      <input type="password" name="${name}" ${reqAttr} class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 pr-10 sm:pr-12 text-sm sm:text-base text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="${placeholder}">
      ${eyeBtn()}
    </div>
  </div>`;
}

// ── Eye toggle ──────────────────────────────────────────────────────────
function toggleEye(btn) {
  const input = btn.parentElement.querySelector('input');
  const closed = btn.querySelector('.eye-closed');
  const open = btn.querySelector('.eye-open');
  if (input.type === 'password') {
    input.type = 'text';
    closed.classList.add('hidden');
    open.classList.remove('hidden');
  } else {
    input.type = 'password';
    closed.classList.remove('hidden');
    open.classList.add('hidden');
  }
}

// ── Load backups on page load ───────────────────────────────────────────
async function loadBackups() {
  try {
    const res = await fetch('/api/backups');
    const backups = await res.json();
    for (const b of backups) {
      const sizeKB = (b.size / 1024).toFixed(1);
      backupOptions += `<option value="${b.path}">${b.name} (${sizeKB} KB)</option>`;
    }
  } catch(e) { /* ignore */ }
}
loadBackups();

// ── Add deploy card ─────────────────────────────────────────────────────

function addDeployCard() {
  deployCounter++;
  const n = deployCounter;
  const container = document.getElementById('deploys-container');
  const card = document.createElement('div');
  card.id = 'deploy-card-' + n;
  card.className = 'bg-slate-900 border border-slate-800 rounded-xl p-4 sm:p-6 shadow-xl';
  card.innerHTML = `
    <div class="flex items-center justify-between mb-6">
      <h2 class="text-lg font-semibold text-slate-100">Deployment #${n}</h2>
      <button type="button" onclick="removeCard(${n})" class="text-slate-500 hover:text-red-400 transition-colors" title="Remove">
        <svg class="w-5 h-5" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12"/></svg>
      </button>
    </div>
    <form class="space-y-6" onsubmit="startDeploy(event, ${n})">
      <fieldset class="space-y-4">
        <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Credentials</legend>
        ${passwordField('do_token', 'DigitalOcean Token', 'dop_v1_...', true)}
        ${passwordField('anthropic_key', 'Anthropic API Key', 'sk-ant-...', true)}
        ${passwordField('openai_key', 'OpenAI API Key', 'sk-...', false)}
        ${passwordField('gemini_key', 'Gemini API Key', 'AI...', false)}
      </fieldset>
      <fieldset class="space-y-4">
        <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Messaging <span class="text-slate-500 normal-case">(optional)</span></legend>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          ${passwordField('whatsapp_phone_number', 'WhatsApp Phone', '+1234567890', false)}
          ${passwordField('telegram_bot_token', 'Telegram Bot Token', '123456:ABC-DEF...', false)}
        </div>
      </fieldset>
      <fieldset class="space-y-4">
        <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Infrastructure</legend>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <label class="block text-sm font-medium text-slate-300 mb-1">Region</label>
            <select name="region" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
              <option value="sgp1" selected>sgp1 (Singapore 1)</option>
              <option value="nyc1">nyc1 (New York 1)</option>
              <option value="nyc3">nyc3 (New York 3)</option>
              <option value="sfo3">sfo3 (San Francisco 3)</option>
              <option value="ams3">ams3 (Amsterdam 3)</option>
              <option value="lon1">lon1 (London 1)</option>
              <option value="fra1">fra1 (Frankfurt 1)</option>
              <option value="tor1">tor1 (Toronto 1)</option>
              <option value="blr1">blr1 (Bangalore 1)</option>
              <option value="syd1">syd1 (Sydney 1)</option>
            </select>
          </div>
          <div>
            <label class="block text-sm font-medium text-slate-300 mb-1">Droplet Size</label>
            <select name="size" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
              <option value="s-1vcpu-1gb">s-1vcpu-1gb (1 vCPU, 1 GB - $6/mo)</option>
              <option value="s-1vcpu-2gb">s-1vcpu-2gb (1 vCPU, 2 GB - $12/mo)</option>
              <option value="s-2vcpu-4gb" selected>s-2vcpu-4gb (2 vCPUs, 4 GB - $24/mo)</option>
              <option value="s-4vcpu-8gb">s-4vcpu-8gb (4 vCPUs, 8 GB - $48/mo)</option>
              <option value="s-8vcpu-16gb">s-8vcpu-16gb (8 vCPUs, 16 GB - $96/mo)</option>
            </select>
          </div>
        </div>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <label class="block text-sm font-medium text-slate-300 mb-1">Hostname</label>
            <input type="text" name="hostname" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="Auto-generated if empty">
          </div>
          <div>
            <label class="block text-sm font-medium text-slate-300 mb-1">Restore Backup</label>
            <select name="backup" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
              ${backupOptions}
            </select>
          </div>
        </div>
      </fieldset>
      <fieldset class="space-y-3">
        <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Options</legend>
        <label class="flex items-center gap-3 cursor-pointer">
          <input type="checkbox" name="enable_backups" class="w-4 h-4 rounded border-slate-600 bg-slate-800 text-blue-500 focus:ring-blue-500 focus:ring-offset-0">
          <span class="text-sm text-slate-300">Enable DigitalOcean automated backups</span>
        </label>
        <label class="flex items-center gap-3 cursor-pointer">
          <input type="checkbox" name="enable_sandbox" class="w-4 h-4 rounded border-slate-600 bg-slate-800 text-blue-500 focus:ring-blue-500 focus:ring-offset-0">
          <span class="text-sm text-slate-300">Enable OpenClaw sandboxing (Docker)</span>
        </label>
        <label class="flex items-center gap-3 cursor-pointer">
          <input type="checkbox" name="tailscale" class="w-4 h-4 rounded border-slate-600 bg-slate-800 text-blue-500 focus:ring-blue-500 focus:ring-offset-0">
          <span class="text-sm text-slate-300">Enable Tailscale VPN</span>
        </label>
      </fieldset>
      <button type="submit" class="deploy-submit-btn w-full bg-blue-600 hover:bg-blue-500 text-white font-semibold py-2.5 sm:py-3 text-sm sm:text-base rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-blue-400 focus:ring-offset-2 focus:ring-offset-slate-900">
        Deploy
      </button>
    </form>
    <div class="deploy-progress hidden mt-6">
      <div class="flex items-center justify-between mb-4">
        <h3 class="text-sm font-semibold text-slate-300">Progress</h3>
        <span class="deploy-badge inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-medium bg-blue-500/20 text-blue-300 border border-blue-500/30">
          <span class="w-2 h-2 rounded-full bg-blue-400 animate-pulse"></span>
          Running
        </span>
      </div>
      <div class="mb-4">
        <div class="flex justify-between text-sm text-slate-400 mb-1">
          <span>Step <span class="deploy-step-current">0</span> / 16</span>
          <span class="deploy-step-percent">0%</span>
        </div>
        <div class="w-full h-2 bg-slate-800 rounded-full overflow-hidden">
          <div class="deploy-progress-bar h-full bg-blue-500 rounded-full transition-all duration-500 ease-out" style="width:0%"></div>
        </div>
      </div>
      <div class="deploy-log-area bg-slate-950 border border-slate-800 rounded-lg p-3 sm:p-4 h-60 sm:h-80 overflow-y-auto font-mono text-xs sm:text-sm leading-relaxed"></div>
      <div class="deploy-summary hidden mt-4 bg-slate-800/50 border border-green-500/30 rounded-lg p-3 sm:p-4">
        <h3 class="text-green-400 font-semibold mb-2 text-sm sm:text-base">Deployment Complete</h3>
        <div class="deploy-summary-content text-xs sm:text-sm text-slate-300 space-y-1 font-mono overflow-x-auto"></div>
      </div>
    </div>
  `;
  container.appendChild(card);
  card.scrollIntoView({ behavior: 'smooth' });
}

function removeCard(n) {
  const card = document.getElementById('deploy-card-' + n);
  if (card) card.remove();
}

// ── Deploy helpers ──────────────────────────────────────────────────────

function panelAppendLog(panel, text, color) {
  const area = panel.querySelector('.deploy-log-area');
  const line = document.createElement('div');
  line.className = color || 'text-slate-300';
  line.textContent = text;
  area.appendChild(line);
  area.scrollTop = area.scrollHeight;
}

function panelUpdateProgress(panel, step) {
  const prev = parseInt(panel.querySelector('.deploy-step-current').textContent) || 0;
  if (step <= prev) return;
  const pct = Math.round((step / TOTAL_STEPS) * 100);
  panel.querySelector('.deploy-step-current').textContent = step;
  panel.querySelector('.deploy-step-percent').textContent = pct + '%';
  panel.querySelector('.deploy-progress-bar').style.width = pct + '%';
}

function panelSetStatus(panel, status) {
  const badge = panel.querySelector('.deploy-badge');
  if (status === 'completed') {
    badge.className = 'deploy-badge inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-medium bg-green-500/20 text-green-300 border border-green-500/30';
    badge.innerHTML = '<span class="w-2 h-2 rounded-full bg-green-400"></span> Completed';
  } else if (status === 'failed') {
    badge.className = 'deploy-badge inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-medium bg-red-500/20 text-red-300 border border-red-500/30';
    badge.innerHTML = '<span class="w-2 h-2 rounded-full bg-red-400"></span> Failed';
  } else {
    badge.className = 'deploy-badge inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-medium bg-blue-500/20 text-blue-300 border border-blue-500/30';
    badge.innerHTML = '<span class="w-2 h-2 rounded-full bg-blue-400 animate-pulse"></span> Running';
  }
}

function panelShowSummary(panel, ip, keyPath, hostname) {
  const card = panel.querySelector('.deploy-summary');
  const content = panel.querySelector('.deploy-summary-content');
  panel.dataset.deployIp = ip;
  panel.dataset.deployKeyPath = keyPath;
  card.classList.remove('hidden');
  content.innerHTML = `
    <p><span class="text-slate-500">Hostname:</span> ${hostname}</p>
    <p><span class="text-slate-500">IP:</span> ${ip}</p>
    <p><span class="text-slate-500">SSH:</span> ssh -i ${keyPath} openclaw@${ip}</p>
    <div class="mt-3 border border-slate-700 rounded-lg p-3">
      <p class="text-slate-300 font-semibold mb-2">Telegram DM Pairing</p>
      <p class="text-xs text-slate-400 mb-2">Message your Telegram bot to get an 8-char pairing code, then approve it here.</p>
      <div class="flex flex-col sm:flex-row gap-2">
        <input type="text" maxlength="16" class="telegram-pairing-code w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-200" placeholder="AB12CD34">
        <button type="button" onclick="approveTelegramPairing(this)" class="telegram-pairing-btn bg-blue-600 hover:bg-blue-500 text-white font-semibold px-4 py-2 rounded-lg whitespace-nowrap">Approve</button>
      </div>
      <div class="telegram-pairing-result text-xs mt-2 text-slate-400"></div>
    </div>
    <div class="mt-3 border border-slate-700 rounded-lg p-3">
      <p class="text-slate-300 font-semibold mb-2">WhatsApp Link QR</p>
      <p class="text-xs text-slate-400 mb-2">Click fetch, then scan the QR from WhatsApp mobile (Linked Devices). Output below is live command output from the droplet.</p>
      <button type="button" onclick="fetchWhatsAppQr(this)" class="whatsapp-qr-btn bg-blue-600 hover:bg-blue-500 text-white font-semibold px-4 py-2 rounded-lg">Fetch QR</button>
      <pre class="whatsapp-qr-output mt-2 bg-slate-900 border border-slate-700 rounded-lg p-2 text-[11px] leading-4 whitespace-pre-wrap text-slate-300 max-h-64 overflow-y-auto"></pre>
    </div>
  `;
}

async function approveTelegramPairing(btn) {
  const panel = btn.closest('[id^="deploy-card-"]');
  if (!panel) return;

  const codeInput = panel.querySelector('.telegram-pairing-code');
  const result = panel.querySelector('.telegram-pairing-result');
  const code = (codeInput.value || '').trim();
  const ip = panel.dataset.deployIp || '';
  const keyPath = panel.dataset.deployKeyPath || '';

  if (!code) {
    result.className = 'telegram-pairing-result text-xs mt-2 text-red-400';
    result.textContent = 'Enter a pairing code first.';
    return;
  }
  if (!ip || !keyPath) {
    result.className = 'telegram-pairing-result text-xs mt-2 text-red-400';
    result.textContent = 'Missing deploy connection details.';
    return;
  }

  const oldText = btn.textContent;
  btn.disabled = true;
  btn.textContent = 'Approving...';
  result.className = 'telegram-pairing-result text-xs mt-2 text-slate-400';
  result.textContent = 'Approving code on droplet...';

  try {
    const res = await fetch('/api/telegram/pairing/approve', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ip: ip,
        ssh_key_path: keyPath,
        pairing_code: code
      })
    });
    const data = await res.json();
    if (res.ok && data.ok) {
      result.className = 'telegram-pairing-result text-xs mt-2 text-green-400';
      result.textContent = data.message || 'Telegram pairing approved.';
      codeInput.value = '';
    } else {
      result.className = 'telegram-pairing-result text-xs mt-2 text-red-400';
      result.textContent = data.message || 'Failed to approve pairing code.';
    }
  } catch (err) {
    result.className = 'telegram-pairing-result text-xs mt-2 text-red-400';
    result.textContent = 'Request failed: ' + err.message;
  } finally {
    btn.disabled = false;
    btn.textContent = oldText;
  }
}

async function fetchWhatsAppQr(btn) {
  const panel = btn.closest('[id^="deploy-card-"]');
  if (!panel) return;

  const output = panel.querySelector('.whatsapp-qr-output');
  const ip = panel.dataset.deployIp || '';
  const keyPath = panel.dataset.deployKeyPath || '';
  if (!ip || !keyPath) {
    output.textContent = 'Missing deploy connection details.';
    return;
  }

  const oldText = btn.textContent;
  btn.disabled = true;
  btn.textContent = 'Fetching...';
  output.textContent = 'Starting WhatsApp login command on droplet...';

  try {
    const res = await fetch('/api/whatsapp/qr', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ip: ip,
        ssh_key_path: keyPath
      })
    });
    const data = await res.json();
    if (res.ok && data.ok) {
      output.textContent = (data.qr_output || '').trim() || 'No QR output returned.';
    } else {
      output.textContent = data.message || 'Failed to fetch WhatsApp QR.';
    }
  } catch (err) {
    output.textContent = 'Request failed: ' + err.message;
  } finally {
    btn.disabled = false;
    btn.textContent = oldText;
  }
}

// ── Deploy ──────────────────────────────────────────────────────────────

async function startDeploy(e, cardNum) {
  e.preventDefault();

  const card = document.getElementById('deploy-card-' + cardNum);
  const form = card.querySelector('form');
  const btn = card.querySelector('.deploy-submit-btn');
  const progressDiv = card.querySelector('.deploy-progress');

  const val = (name) => (form.querySelector(`[name="${name}"]`) || {}).value || '';
  const body = {
    do_token: val('do_token'),
    anthropic_key: val('anthropic_key'),
    openai_key: val('openai_key'),
    gemini_key: val('gemini_key'),
    whatsapp_phone_number: val('whatsapp_phone_number'),
    telegram_bot_token: val('telegram_bot_token'),
    region: val('region'),
    size: val('size'),
    hostname: val('hostname'),
    backup: val('backup'),
    enable_backups: form.querySelector('[name="enable_backups"]').checked,
    enable_sandbox: form.querySelector('[name="enable_sandbox"]').checked,
    tailscale: form.querySelector('[name="tailscale"]').checked,
  };

  // Disable button and show progress
  panelSetStatus(card, 'running');
  card.querySelector('.deploy-step-current').textContent = '0';
  card.querySelector('.deploy-step-percent').textContent = '0%';
  card.querySelector('.deploy-progress-bar').style.width = '0%';
  card.querySelector('.deploy-log-area').innerHTML = '';
  card.querySelector('.deploy-summary').classList.add('hidden');
  card.querySelector('.deploy-summary-content').innerHTML = '';
  delete card.dataset.deployIp;
  delete card.dataset.deployKeyPath;
  btn.disabled = true;
  btn.textContent = 'Deploying...';
  btn.className = btn.className.replace('bg-blue-600 hover:bg-blue-500', 'bg-slate-700 cursor-not-allowed');
  progressDiv.classList.remove('hidden');

  try {
    const res = await fetch('/api/deploy', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const data = await res.json();
    const deployId = data.deploy_id;

    const evtSource = new EventSource(`/api/deploy/${deployId}/events`);
    evtSource.onmessage = function(event) {
      const msg = event.data;

      if (msg.startsWith('DEPLOY_COMPLETE:')) {
        const parts = msg.split(':');
        const ip = parts[1];
        const keyPath = parts[2];
        const hostname = parts[3];
        panelSetStatus(card, 'completed');
        panelUpdateProgress(card, TOTAL_STEPS);
        panelAppendLog(card, 'Deploy completed successfully!', 'text-green-400 font-semibold');
        evtSource.close();
        panelShowSummary(card, ip, keyPath, hostname);
        return;
      }

      if (msg.startsWith('DEPLOY_ERROR:')) {
        const err = msg.substring(13);
        panelSetStatus(card, 'failed');
        panelAppendLog(card, 'ERROR: ' + err, 'text-red-400 font-semibold');
        evtSource.close();
        btn.disabled = false;
        btn.textContent = 'Retry Deploy';
        btn.className = btn.className.replace('bg-slate-700 cursor-not-allowed', 'bg-blue-600 hover:bg-blue-500');
        return;
      }

      const match = msg.match(/\[Step (\d+)\/16\]/);
      if (match) {
        panelUpdateProgress(card, parseInt(match[1]));
      }

      const trimmed = msg.trim();
      if (!trimmed) return;
      let color = 'text-slate-400';
      if (trimmed.startsWith('[Step')) color = 'text-blue-300 font-medium';
      else if (trimmed.startsWith('  ')) color = 'text-slate-400';
      panelAppendLog(card, trimmed, color);
    };

    evtSource.onerror = function() {
      evtSource.close();
    };

  } catch(err) {
    panelSetStatus(card, 'failed');
    panelAppendLog(card, 'Failed to start deploy: ' + err.message, 'text-red-400');
    btn.disabled = false;
    btn.textContent = 'Retry Deploy';
    btn.className = btn.className.replace('bg-slate-700 cursor-not-allowed', 'bg-blue-600 hover:bg-blue-500');
  }
}

// Auto-add the first deployment card
addDeployCard();
</script>
</body>
</html>
"##;
