use crate::commands::deploy::{self, DeployParams};
use crate::config;
use axum::extract::{Path, State};
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

// ── Entry point ─────────────────────────────────────────────────────────────

pub async fn run(port: u16) -> anyhow::Result<()> {
    let jobs: Jobs = Arc::new(RwLock::new(HashMap::new()));

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/assets/mascot.jpg", get(mascot_handler))
        .route("/api/backups", get(list_backups_handler))
        .route("/api/deploy", post(start_deploy_handler))
        .route("/api/deploy/{id}/events", get(deploy_events_handler))
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

// ── Helpers ─────────────────────────────────────────────────────────────────

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
  #log-area::-webkit-scrollbar { width: 8px; }
  #log-area::-webkit-scrollbar-track { background: #0f172a; }
  #log-area::-webkit-scrollbar-thumb { background: #334155; border-radius: 4px; }
  .eye-btn { position:absolute; right:12px; top:50%; transform:translateY(-50%); cursor:pointer; color:#94a3b8; }
  .eye-btn:hover { color:#e2e8f0; }
</style>
</head>
<body class="bg-slate-950 text-slate-200 min-h-screen">

<!-- Header -->
<header class="border-b border-slate-800 bg-slate-950/80 backdrop-blur sticky top-0 z-10">
  <div class="max-w-4xl mx-auto px-6 py-4 flex items-center gap-3">
    <svg class="w-8 h-8 text-blue-400" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M5 12h14M12 5l7 7-7 7"/></svg>
    <h1 class="text-xl font-bold tracking-tight">ClawMacToDO</h1>
    <span class="text-sm text-slate-500 ml-2">Deploy OpenClaw to DigitalOcean</span>
  </div>
</header>

<main class="max-w-4xl mx-auto px-6 py-8">

<!-- Mascot -->
<div class="flex justify-center mb-6">
  <img src="/assets/mascot.jpg" alt="ClawMacToDO Mascot" class="rounded-xl shadow-lg max-w-sm w-full">
</div>

<!-- Deploy Form Card -->
<div id="form-card" class="bg-slate-900 border border-slate-800 rounded-xl p-6 shadow-xl">
  <h2 class="text-lg font-semibold mb-6 text-slate-100">Deploy Configuration</h2>

  <form id="deploy-form" class="space-y-6" onsubmit="startDeploy(event)">
    <!-- Credentials -->
    <fieldset class="space-y-4">
      <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Credentials</legend>

      <div>
        <label class="block text-sm font-medium text-slate-300 mb-1" for="do_token">DigitalOcean Token <span class="text-red-400">*</span></label>
        <div class="relative">
          <input type="password" id="do_token" name="do_token" required class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 pr-12 text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="dop_v1_...">
          <button type="button" class="eye-btn" onclick="toggleEye(this)">
            <svg class="w-5 h-5 eye-closed" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"/></svg>
            <svg class="w-5 h-5 eye-open hidden" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>
          </button>
        </div>
      </div>

      <div>
        <label class="block text-sm font-medium text-slate-300 mb-1" for="anthropic_key">Anthropic API Key <span class="text-red-400">*</span></label>
        <div class="relative">
          <input type="password" id="anthropic_key" name="anthropic_key" required class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 pr-12 text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="sk-ant-...">
          <button type="button" class="eye-btn" onclick="toggleEye(this)">
            <svg class="w-5 h-5 eye-closed" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"/></svg>
            <svg class="w-5 h-5 eye-open hidden" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>
          </button>
        </div>
      </div>

      <div>
        <label class="block text-sm font-medium text-slate-300 mb-1" for="openai_key">OpenAI API Key <span class="text-slate-500">(optional)</span></label>
        <div class="relative">
          <input type="password" id="openai_key" name="openai_key" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 pr-12 text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="sk-...">
          <button type="button" class="eye-btn" onclick="toggleEye(this)">
            <svg class="w-5 h-5 eye-closed" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"/></svg>
            <svg class="w-5 h-5 eye-open hidden" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>
          </button>
        </div>
      </div>

      <div>
        <label class="block text-sm font-medium text-slate-300 mb-1" for="gemini_key">Gemini API Key <span class="text-slate-500">(optional)</span></label>
        <div class="relative">
          <input type="password" id="gemini_key" name="gemini_key" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 pr-12 text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="AI...">
          <button type="button" class="eye-btn" onclick="toggleEye(this)">
            <svg class="w-5 h-5 eye-closed" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"/></svg>
            <svg class="w-5 h-5 eye-open hidden" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>
          </button>
        </div>
      </div>
    </fieldset>

    <!-- Messaging -->
    <fieldset class="space-y-4">
      <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Messaging <span class="text-slate-500 normal-case">(optional)</span></legend>

      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <div>
          <label class="block text-sm font-medium text-slate-300 mb-1" for="whatsapp_phone_number">WhatsApp Phone</label>
          <div class="relative">
            <input type="password" id="whatsapp_phone_number" name="whatsapp_phone_number" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 pr-12 text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="+1234567890">
            <button type="button" class="eye-btn" onclick="toggleEye(this)">
              <svg class="w-5 h-5 eye-closed" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"/></svg>
              <svg class="w-5 h-5 eye-open hidden" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>
            </button>
          </div>
        </div>
        <div>
          <label class="block text-sm font-medium text-slate-300 mb-1" for="telegram_bot_token">Telegram Bot Token</label>
          <div class="relative">
            <input type="password" id="telegram_bot_token" name="telegram_bot_token" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 pr-12 text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="123456:ABC-DEF...">
            <button type="button" class="eye-btn" onclick="toggleEye(this)">
              <svg class="w-5 h-5 eye-closed" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"/></svg>
              <svg class="w-5 h-5 eye-open hidden" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>
            </button>
          </div>
        </div>
      </div>
    </fieldset>

    <!-- Infrastructure -->
    <fieldset class="space-y-4">
      <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Infrastructure</legend>

      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <div>
          <label class="block text-sm font-medium text-slate-300 mb-1" for="region">Region</label>
          <select id="region" name="region" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
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
          <label class="block text-sm font-medium text-slate-300 mb-1" for="size">Droplet Size</label>
          <select id="size" name="size" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
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
          <label class="block text-sm font-medium text-slate-300 mb-1" for="hostname">Hostname</label>
          <input type="text" id="hostname" name="hostname" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="Auto-generated if empty">
        </div>

        <div>
          <label class="block text-sm font-medium text-slate-300 mb-1" for="backup">Restore Backup</label>
          <select id="backup" name="backup" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
            <option value="none">None</option>
          </select>
        </div>
      </div>
    </fieldset>

    <!-- Toggles -->
    <fieldset class="space-y-3">
      <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Options</legend>

      <label class="flex items-center gap-3 cursor-pointer">
        <input type="checkbox" id="enable_backups" name="enable_backups" class="w-4 h-4 rounded border-slate-600 bg-slate-800 text-blue-500 focus:ring-blue-500 focus:ring-offset-0">
        <span class="text-sm text-slate-300">Enable DigitalOcean automated backups</span>
      </label>
    </fieldset>

    <button type="submit" id="deploy-btn" class="w-full bg-blue-600 hover:bg-blue-500 text-white font-semibold py-3 rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-blue-400 focus:ring-offset-2 focus:ring-offset-slate-900">
      Deploy
    </button>
  </form>
</div>

<!-- Progress Panel (hidden until deploy starts) -->
<div id="progress-panel" class="hidden mt-6 bg-slate-900 border border-slate-800 rounded-xl p-6 shadow-xl">
  <!-- Status header -->
  <div class="flex items-center justify-between mb-4">
    <h2 class="text-lg font-semibold text-slate-100">Deploy Progress</h2>
    <span id="status-badge" class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-medium bg-blue-500/20 text-blue-300 border border-blue-500/30">
      <span class="w-2 h-2 rounded-full bg-blue-400 animate-pulse"></span>
      Running
    </span>
  </div>

  <!-- Progress bar -->
  <div class="mb-4">
    <div class="flex justify-between text-sm text-slate-400 mb-1">
      <span>Step <span id="step-current">0</span> / 12</span>
      <span id="step-percent">0%</span>
    </div>
    <div class="w-full h-2 bg-slate-800 rounded-full overflow-hidden">
      <div id="progress-bar" class="h-full bg-blue-500 rounded-full transition-all duration-500 ease-out" style="width:0%"></div>
    </div>
  </div>

  <!-- Log area -->
  <div id="log-area" class="bg-slate-950 border border-slate-800 rounded-lg p-4 h-80 overflow-y-auto font-mono text-sm leading-relaxed"></div>

  <!-- Summary card (hidden until complete) -->
  <div id="summary-card" class="hidden mt-4 bg-slate-800/50 border border-green-500/30 rounded-lg p-4">
    <h3 class="text-green-400 font-semibold mb-2">Deployment Complete</h3>
    <div id="summary-content" class="text-sm text-slate-300 space-y-1 font-mono"></div>
  </div>
</div>

</main>

<script>
const TOTAL_STEPS = 12;

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
    const sel = document.getElementById('backup');
    for (const b of backups) {
      const opt = document.createElement('option');
      opt.value = b.path;
      const sizeKB = (b.size / 1024).toFixed(1);
      opt.textContent = `${b.name} (${sizeKB} KB)`;
      sel.appendChild(opt);
    }
  } catch(e) { /* ignore */ }
}
loadBackups();

// ── Deploy ──────────────────────────────────────────────────────────────
let currentStep = 0;

function appendLog(text, color) {
  const area = document.getElementById('log-area');
  const line = document.createElement('div');
  line.className = color || 'text-slate-300';
  line.textContent = text;
  area.appendChild(line);
  area.scrollTop = area.scrollHeight;
}

function updateProgress(step) {
  if (step <= currentStep) return;
  currentStep = step;
  const pct = Math.round((step / TOTAL_STEPS) * 100);
  document.getElementById('step-current').textContent = step;
  document.getElementById('step-percent').textContent = pct + '%';
  document.getElementById('progress-bar').style.width = pct + '%';
}

function resetDeployBtn() {
  const btn = document.getElementById('deploy-btn');
  btn.disabled = false;
  btn.textContent = 'Deploy';
  btn.className = btn.className.replace('bg-slate-700 cursor-not-allowed', 'bg-blue-600 hover:bg-blue-500');
  currentStep = 0;
}

function setStatus(status) {
  const badge = document.getElementById('status-badge');
  if (status === 'completed') {
    badge.className = 'inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-medium bg-green-500/20 text-green-300 border border-green-500/30';
    badge.innerHTML = '<span class="w-2 h-2 rounded-full bg-green-400"></span> Completed';
  } else if (status === 'failed') {
    badge.className = 'inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-medium bg-red-500/20 text-red-300 border border-red-500/30';
    badge.innerHTML = '<span class="w-2 h-2 rounded-full bg-red-400"></span> Failed';
  }
}

async function startDeploy(e) {
  e.preventDefault();

  const form = document.getElementById('deploy-form');
  const btn = document.getElementById('deploy-btn');
  btn.disabled = true;
  btn.textContent = 'Deploying...';
  btn.className = btn.className.replace('bg-blue-600 hover:bg-blue-500', 'bg-slate-700 cursor-not-allowed');

  const body = {
    do_token: form.do_token.value,
    anthropic_key: form.anthropic_key.value,
    openai_key: form.openai_key.value,
    gemini_key: form.gemini_key.value,
    whatsapp_phone_number: form.whatsapp_phone_number.value,
    telegram_bot_token: form.telegram_bot_token.value,
    region: form.region.value,
    size: form.size.value,
    hostname: form.hostname.value,
    backup: form.backup.value,
    enable_backups: form.enable_backups.checked,
  };

  // Show progress panel
  document.getElementById('progress-panel').classList.remove('hidden');
  document.getElementById('progress-panel').scrollIntoView({ behavior: 'smooth' });

  try {
    const res = await fetch('/api/deploy', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const data = await res.json();
    const deployId = data.deploy_id;

    // Open SSE
    const evtSource = new EventSource(`/api/deploy/${deployId}/events`);
    evtSource.onmessage = function(event) {
      const msg = event.data;

      // Check for completion signal
      if (msg.startsWith('DEPLOY_COMPLETE:')) {
        const parts = msg.split(':');
        const ip = parts[1];
        const keyPath = parts[2];
        const hostname = parts[3];
        setStatus('completed');
        updateProgress(TOTAL_STEPS);
        appendLog('Deploy completed successfully!', 'text-green-400 font-semibold');
        evtSource.close();
        showSummary(ip, keyPath, hostname);
        return;
      }

      if (msg.startsWith('DEPLOY_ERROR:')) {
        const err = msg.substring(13);
        setStatus('failed');
        appendLog('ERROR: ' + err, 'text-red-400 font-semibold');
        evtSource.close();
        resetDeployBtn();
        return;
      }

      // Parse step number from "[Step N/12]"
      const match = msg.match(/\[Step (\d+)\/12\]/);
      if (match) {
        updateProgress(parseInt(match[1]));
      }

      // Color code
      const trimmed = msg.trim();
      if (!trimmed) return;
      let color = 'text-slate-400';
      if (trimmed.startsWith('[Step')) color = 'text-blue-300 font-medium';
      else if (trimmed.startsWith('  ')) color = 'text-slate-400';
      appendLog(trimmed, color);
    };

    evtSource.onerror = function() {
      evtSource.close();
    };

  } catch(err) {
    setStatus('failed');
    appendLog('Failed to start deploy: ' + err.message, 'text-red-400');
    resetDeployBtn();
  }
}

function showSummary(ip, keyPath, hostname) {
  const card = document.getElementById('summary-card');
  const content = document.getElementById('summary-content');
  card.classList.remove('hidden');
  content.innerHTML = `
    <p><span class="text-slate-500">Hostname:</span> ${hostname}</p>
    <p><span class="text-slate-500">IP:</span> ${ip}</p>
    <p><span class="text-slate-500">Gateway:</span> http://${ip}:18789</p>
    <p><span class="text-slate-500">SSH:</span> ssh -i ${keyPath} root@${ip}</p>
  `;
}
</script>
</body>
</html>
"##;
