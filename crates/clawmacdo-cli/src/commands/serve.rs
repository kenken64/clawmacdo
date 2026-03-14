use crate::commands::deploy::{self, DeployParams};
use crate::commands::docker_fix;
use crate::commands::whatsapp;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{delete, get, post};
use axum::Router;
use clawmacdo_core::config;
use clawmacdo_db as db;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_async, ssh_as_openclaw_with_user_async,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;

// ── Shared state ────────────────────────────────────────────────────────────

type Db = Arc<Mutex<rusqlite::Connection>>;
type Jobs = Arc<RwLock<HashMap<String, DeployJob>>>;

#[derive(Clone)]
struct AppState {
    jobs: Jobs,
    db: Db,
}

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
    customer_name: String,
    customer_email: String,
    #[serde(default = "default_provider")]
    provider: String,
    do_token: String,
    #[serde(default)]
    tencent_secret_id: String,
    #[serde(default)]
    tencent_secret_key: String,
    #[serde(default)]
    aws_access_key_id: String,
    #[serde(default)]
    aws_secret_access_key: String,
    #[serde(default)]
    aws_region: String,
    #[serde(default)]
    azure_tenant_id: String,
    #[serde(default)]
    azure_subscription_id: String,
    #[serde(default)]
    azure_client_id: String,
    #[serde(default)]
    azure_client_secret: String,
    #[serde(default)]
    anthropic_key: String,
    #[serde(default)]
    openai_key: String,
    #[serde(default)]
    gemini_key: String,
    #[serde(default = "default_primary_model")]
    primary_model: String,
    #[serde(default)]
    failover_1: String,
    #[serde(default)]
    failover_2: String,
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
    #[serde(default)]
    tailscale_auth_key: String,
    #[serde(default = "default_profile")]
    profile: String,
}

fn default_provider() -> String {
    "digitalocean".to_string()
}

fn default_primary_model() -> String {
    "anthropic".to_string()
}

fn default_profile() -> String {
    "full".to_string()
}

#[derive(Serialize)]
struct DeployResponse {
    deploy_id: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    message: String,
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
    #[serde(default)]
    provider: Option<String>,
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

#[derive(Deserialize)]
struct WhatsAppRepairRequest {
    ip: String,
    ssh_key_path: String,
}

#[derive(Serialize)]
struct WhatsAppRepairResponse {
    ok: bool,
    message: String,
    repair_output: String,
}

#[derive(Deserialize)]
struct DockerFixRequest {
    ip: String,
    ssh_key_path: String,
    #[serde(default)]
    provider: Option<String>,
}

#[derive(Serialize)]
struct DockerFixResponse {
    ok: bool,
    message: String,
    fix_output: String,
}

#[derive(Deserialize)]
struct ListDeploymentsQuery {
    #[serde(default = "default_page")]
    page: u32,
}

fn default_page() -> u32 {
    1
}

#[derive(Serialize)]
struct ListDeploymentsResponse {
    deployments: Vec<db::DeploymentRow>,
    total: u32,
    page: u32,
    per_page: u32,
    total_pages: u32,
}

#[derive(Serialize)]
struct DeleteResponse {
    ok: bool,
}

#[derive(Serialize)]
struct ConfigResponse {
    dry_run: bool,
}

// ── Entry point ─────────────────────────────────────────────────────────────

/// RRun.
pub async fn run(port: u16) -> anyhow::Result<()> {
    let conn = db::init_db()?;
    let db: Db = Arc::new(Mutex::new(conn));
    let jobs: Jobs = Arc::new(RwLock::new(HashMap::new()));
    let state = AppState { jobs, db };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/assets/mascot.jpg", get(mascot_handler))
        .route("/api/backups", get(list_backups_handler))
        .route("/api/deploy", post(start_deploy_handler))
        .route("/api/deploy/{id}/events", get(deploy_events_handler))
        .route(
            "/api/telegram/pairing/approve",
            post(approve_telegram_pairing_handler),
        )
        .route("/api/agent/docker-fix", post(repair_agent_docker_handler))
        .route("/api/whatsapp/repair", post(repair_whatsapp_handler))
        .route("/api/whatsapp/qr", post(fetch_whatsapp_qr_handler))
        .route("/api/deployments", get(list_deployments_handler))
        .route("/api/deployments/{id}", delete(delete_deployment_handler))
        .route("/api/config", get(config_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    println!("ClawMacToDO web UI running at http://localhost:{port}");
    println!("Press Ctrl+C to stop.\n");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ── Route handlers ──────────────────────────────────────────────────────────

/// IIndex handler.
async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// MMascot handler.
async fn mascot_handler() -> impl IntoResponse {
    const MASCOT: &[u8] = include_bytes!("../../../../assets/mascot.jpg");
    (
        [
            ("content-type", "image/jpeg"),
            ("cache-control", "public, max-age=86400"),
        ],
        MASCOT,
    )
}

/// LList backups handler.
async fn list_backups_handler() -> impl IntoResponse {
    let entries = list_backup_files().unwrap_or_default();
    Json(entries)
}

/// SStart deploy handler.
async fn start_deploy_handler(
    State(state): State<AppState>,
    Json(req): Json<DeployRequest>,
) -> impl IntoResponse {
    let jobs = state.jobs;
    let db = state.db;
    if req.tailscale && req.tailscale_auth_key.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                message: "Tailscale auth key is required when Tailscale is enabled.".into(),
            }),
        )
            .into_response();
    }

    // Validate primary model's API key is present
    let primary_key_present = match req.primary_model.as_str() {
        "anthropic" => !req.anthropic_key.trim().is_empty(),
        "openai" => !req.openai_key.trim().is_empty(),
        "gemini" => !req.gemini_key.trim().is_empty(),
        _ => false,
    };
    if !primary_key_present {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                message: format!(
                    "API key for primary model '{}' is required.",
                    req.primary_model
                ),
            }),
        )
            .into_response();
    }

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
    let db_clone = db.clone();
    let customer_name = req.customer_name.clone();
    let customer_email = req.customer_email.clone();

    tokio::spawn(async move {
        let provider_str = req.provider.clone();
        let region_str = req.region.clone();
        let size_str = req.size.clone();
        let hostname_str = req.hostname.clone();

        let params = DeployParams {
            customer_name: customer_name.clone(),
            customer_email: customer_email.clone(),
            provider: req.provider,
            do_token: req.do_token,
            tencent_secret_id: req.tencent_secret_id,
            tencent_secret_key: req.tencent_secret_key,
            aws_access_key_id: req.aws_access_key_id,
            aws_secret_access_key: req.aws_secret_access_key,
            aws_region: req.aws_region,
            azure_tenant_id: req.azure_tenant_id,
            azure_subscription_id: req.azure_subscription_id,
            azure_client_id: req.azure_client_id,
            azure_client_secret: req.azure_client_secret,
            anthropic_key: req.anthropic_key,
            openai_key: req.openai_key,
            gemini_key: req.gemini_key,
            whatsapp_phone_number: req.whatsapp_phone_number,
            telegram_bot_token: req.telegram_bot_token,
            region: if req.region.is_empty() {
                None
            } else {
                Some(req.region)
            },
            size: if req.size.is_empty() {
                None
            } else {
                Some(req.size)
            },
            hostname: if req.hostname.is_empty() {
                None
            } else {
                Some(req.hostname)
            },
            backup,
            enable_backups: req.enable_backups,
            enable_sandbox: req.enable_sandbox,
            tailscale: req.tailscale,
            tailscale_auth_key: if req.tailscale_auth_key.trim().is_empty() {
                None
            } else {
                Some(req.tailscale_auth_key)
            },
            primary_model: req.primary_model,
            failover_1: req.failover_1,
            failover_2: req.failover_2,
            profile: req.profile,
            non_interactive: true,
            progress_tx: Some(tx.clone()),
            db: Some(db_clone.clone()),
        };

        // Insert deployment record into SQLite
        if let Ok(conn) = db_clone.lock() {
            let _ = db::insert_deployment(
                &conn,
                &id,
                &customer_name,
                &customer_email,
                &provider_str,
                &region_str,
                &size_str,
                &hostname_str,
            );
        }

        // Dry-run mode: simulate deploy without real cloud calls
        if is_dry_run() {
            let _ = tx.send("[Dry-run] Deploy simulation started".to_string());
            let steps = [
                "[Step 1/16] Resolving parameters...",
                "[Step 2/16] Generating SSH key pair...",
                "[Step 3/16] Uploading SSH public key (skipped — dry-run)...",
                "[Step 4/16] Creating instance (skipped — dry-run)...",
                "[Step 5/16] Waiting for instance (skipped — dry-run)...",
                "[Step 6/16] Waiting for SSH (skipped — dry-run)...",
                "[Step 7/16] Cloud-init (skipped — dry-run)...",
                "[Step 8/16] Backup restore (skipped — dry-run)...",
                "[Step 9/16] Provision step 1 (skipped — dry-run)...",
                "[Step 10/16] Provision step 2 (skipped — dry-run)...",
                "[Step 11/16] Provision step 3 (skipped — dry-run)...",
                "[Step 12/16] Provision step 4 (skipped — dry-run)...",
                "[Step 13/16] Provision step 5 (skipped — dry-run)...",
                "[Step 14/16] Provision step 6 (skipped — dry-run)...",
                "[Step 15/16] Starting gateway (skipped — dry-run)...",
                "[Step 16/16] Saving deploy record...",
            ];
            for step in &steps {
                let _ = tx.send(step.to_string());
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let dry_hostname = if hostname_str.is_empty() {
                format!("openclaw-{}", &id[..8])
            } else {
                hostname_str.clone()
            };
            if let Ok(conn) = db_clone.lock() {
                let _ = db::update_deployment_status(
                    &conn,
                    &id,
                    "dry-run",
                    Some("0.0.0.0"),
                    Some(&dry_hostname),
                );
            }
            let payload = serde_json::json!({
                "ip": "0.0.0.0",
                "ssh_key_path": "(dry-run)",
                "hostname": dry_hostname
            })
            .to_string();
            let _ = tx.send(format!("DEPLOY_COMPLETE_JSON:{payload}"));
            if let Some(job) = jobs_clone.write().await.get_mut(&id) {
                job.status = JobStatus::Completed;
            }
            return;
        }

        let result = deploy::run(params).await;

        let final_status = match &result {
            Ok(record) => {
                if let Ok(conn) = db_clone.lock() {
                    let _ = db::update_deployment_status(
                        &conn,
                        &id,
                        "completed",
                        Some(&record.ip_address),
                        Some(&record.hostname),
                    );
                }
                let payload = serde_json::json!({
                    "ip": record.ip_address,
                    "ssh_key_path": record.ssh_key_path,
                    "hostname": record.hostname
                })
                .to_string();
                let _ = tx.send(format!("DEPLOY_COMPLETE_JSON:{payload}"));
                JobStatus::Completed
            }
            Err(e) => {
                if let Ok(conn) = db_clone.lock() {
                    let _ = db::update_deployment_status(&conn, &id, "failed", None, None);
                }
                let _ = tx.send(format!("DEPLOY_ERROR:{e:#}"));
                JobStatus::Failed
            }
        };

        if let Some(job) = jobs_clone.write().await.get_mut(&id) {
            job.status = final_status;
        }
    });

    Json(DeployResponse { deploy_id }).into_response()
}

/// DDeploy events handler.
async fn deploy_events_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let jobs = state.jobs;
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

/// AApprove telegram pairing handler.
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

    let ssh_user = ssh_user_for_provider(req.provider.as_deref());
    let result = if ssh_user == "root" {
        ssh_as_openclaw_async(&ip, &key, &cmd).await
    } else {
        ssh_as_openclaw_with_user_async(&ip, &key, &cmd, ssh_user).await
    };
    match result {
        Ok(_out) => (
            StatusCode::OK,
            Json(TelegramPairingApproveResponse {
                ok: true,
                message: "Telegram pairing approved. Send a message to your bot to start chatting."
                    .into(),
            }),
        ),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(TelegramPairingApproveResponse {
                ok: false,
                message: format!("Failed to approve pairing: {e}"),
            }),
        ),
    }
}

/// FFetch whatsapp qr handler.
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
        Ok(out) => {
            let lowered = out.to_ascii_lowercase();
            if lowered.contains("unsupported channel: whatsapp")
                || lowered.contains("unsupported channel whatsapp")
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(WhatsAppQrResponse {
                        ok: false,
                        message: "This OpenClaw install does not support the WhatsApp channel. Click 'Install/Repair WhatsApp Support' first, then fetch QR again.".into(),
                        qr_output: out,
                    }),
                );
            }

            (
                StatusCode::OK,
                Json(WhatsAppQrResponse {
                    ok: true,
                    message: "WhatsApp login output captured.".into(),
                    qr_output: out,
                }),
            )
        }
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

/// RRepair whatsapp handler.
async fn repair_whatsapp_handler(Json(req): Json<WhatsAppRepairRequest>) -> impl IntoResponse {
    let ip = req.ip.trim().to_string();
    let key_path = req.ssh_key_path.trim().to_string();
    if ip.is_empty() || key_path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(WhatsAppRepairResponse {
                ok: false,
                message: "Missing IP or SSH key path.".into(),
                repair_output: String::new(),
            }),
        );
    }

    let key = PathBuf::from(key_path);
    match whatsapp::repair_support(&ip, &key).await {
        Ok(result) => {
            let message = if result.supported {
                "Repair completed. WhatsApp channel appears available now.".to_string()
            } else {
                "Repair completed, but WhatsApp is still unsupported on this OpenClaw build."
                    .to_string()
            };
            let code = if result.supported {
                StatusCode::OK
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                code,
                Json(WhatsAppRepairResponse {
                    ok: result.supported,
                    message,
                    repair_output: result.output,
                }),
            )
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(WhatsAppRepairResponse {
                ok: false,
                message: format!("Failed to run WhatsApp repair: {e}"),
                repair_output: String::new(),
            }),
        ),
    }
}

async fn repair_agent_docker_handler(Json(req): Json<DockerFixRequest>) -> impl IntoResponse {
    let ip = req.ip.trim().to_string();
    let key_path = req.ssh_key_path.trim().to_string();
    if ip.is_empty() || key_path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(DockerFixResponse {
                ok: false,
                message: "Missing IP or SSH key path.".into(),
                fix_output: String::new(),
            }),
        );
    }

    let key = PathBuf::from(key_path);
    let ssh_user = ssh_user_for_provider(req.provider.as_deref());
    match docker_fix::repair_access(&ip, &key, ssh_user).await {
        Ok(result) => {
            let code = if result.ok {
                StatusCode::OK
            } else {
                StatusCode::BAD_REQUEST
            };
            let message = if result.ok {
                "Docker access repair completed. Try messaging the bot again.".to_string()
            } else {
                "Repair ran but Docker/gateway checks still report an issue.".to_string()
            };
            (
                code,
                Json(DockerFixResponse {
                    ok: result.ok,
                    message,
                    fix_output: result.output,
                }),
            )
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(DockerFixResponse {
                ok: false,
                message: format!("Failed to run Docker access repair: {e}"),
                fix_output: String::new(),
            }),
        ),
    }
}

// ── Deployments / Config API handlers ────────────────────────────────────────

fn is_dry_run() -> bool {
    matches!(
        std::env::var("CLAWMACDO_DRY_RUN").as_deref(),
        Ok("true") | Ok("1")
    )
}

async fn list_deployments_handler(
    State(state): State<AppState>,
    Query(q): Query<ListDeploymentsQuery>,
) -> impl IntoResponse {
    let page = if q.page == 0 { 1 } else { q.page };
    let per_page: u32 = 20;
    let conn = state.db.lock().unwrap();
    match db::list_deployments_paginated(&conn, page, per_page) {
        Ok((deployments, total)) => {
            let total_pages = total.div_ceil(per_page);
            Json(ListDeploymentsResponse {
                deployments,
                total,
                page,
                per_page,
                total_pages,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                message: format!("Failed to list deployments: {e}"),
            }),
        )
            .into_response(),
    }
}

async fn delete_deployment_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match db::delete_deployment(&conn, &id) {
        Ok(_) => Json(DeleteResponse { ok: true }).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                message: format!("Failed to delete deployment: {e}"),
            }),
        )
            .into_response(),
    }
}

async fn config_handler() -> impl IntoResponse {
    Json(ConfigResponse {
        dry_run: is_dry_run(),
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Resolve the SSH username from the provider string.
/// Lightsail uses "ubuntu"; all others default to "root".
fn ssh_user_for_provider(provider: Option<&str>) -> &str {
    match provider {
        Some(p) if p.eq_ignore_ascii_case("lightsail") => "ubuntu",
        Some(p) if p.eq_ignore_ascii_case("azure") => "azureuser",
        _ => "root",
    }
}

/// NNormalize pairing code.
fn normalize_pairing_code(raw: &str) -> Option<String> {
    let code = raw.trim().to_ascii_uppercase();
    if code.len() == 8 && code.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(code)
    } else {
        None
    }
}

/// LList backup files.
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
  <div class="max-w-screen-2xl mx-auto px-4 sm:px-8 lg:px-12 py-3 sm:py-4 flex items-center gap-2 sm:gap-3">
    <svg class="w-7 h-7 sm:w-8 sm:h-8 text-blue-400 shrink-0" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M5 12h14M12 5l7 7-7 7"/></svg>
    <h1 class="text-lg sm:text-xl font-bold tracking-tight">ClawMacToDO</h1>
    <span class="text-xs sm:text-sm text-slate-500 ml-1 sm:ml-2 hidden sm:inline">Deploy OpenClaw to the Cloud</span>
    <span class="ml-auto text-xs text-slate-600 font-mono hidden md:inline">v0.11.0</span>
  </div>
  <!-- Tab bar -->
  <div class="max-w-screen-2xl mx-auto px-4 sm:px-8 lg:px-12 flex gap-0">
    <button id="tab-deploy" onclick="switchTab('deploy')" class="px-4 py-2 text-sm font-medium border-b-2 border-blue-500 text-blue-400 transition-colors">Deploy</button>
    <button id="tab-deployments" onclick="switchTab('deployments')" class="px-4 py-2 text-sm font-medium border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition-colors">Deployments</button>
  </div>
</header>

<main class="max-w-screen-2xl mx-auto px-4 sm:px-8 lg:px-12 py-6 sm:py-8">

<!-- Dry-run banner (shared) -->
<div id="dry-run-banner" class="hidden mb-4 bg-yellow-500/10 border border-yellow-500/30 text-yellow-300 rounded-lg px-4 py-2 text-sm font-medium">
  Dry-run mode — deployments are simulated (no real cloud API calls)
</div>

<!-- ═══ Deploy view ═══ -->
<div id="deploy-view">

<!-- Hero -->
<div class="flex flex-col sm:flex-row items-center gap-4 sm:gap-6 mb-8 bg-slate-900/50 border border-slate-800 rounded-xl p-4 sm:p-6">
  <img src="/assets/mascot.jpg" alt="ClawMacToDO Mascot" class="rounded-lg shadow-lg w-24 h-24 sm:w-32 sm:h-32 object-cover shrink-0">
  <div class="flex-1 text-center sm:text-left">
    <h2 class="text-xl sm:text-2xl font-bold text-slate-100 mb-1">Cloud Deployment Console</h2>
    <p class="text-sm text-slate-400 mb-4">Provision OpenClaw instances across DigitalOcean, AWS Lightsail, Tencent Cloud, and Microsoft Azure.</p>
    <div class="flex flex-col sm:flex-row gap-2">
      <button type="button" onclick="addDeployCard()" class="bg-blue-600 hover:bg-blue-500 text-white font-semibold py-2 px-5 text-sm rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-blue-400 focus:ring-offset-2 focus:ring-offset-slate-900 flex items-center justify-center gap-2">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15"/></svg>
        New Deployment
      </button>
      <button type="button" onclick="resetSavedDeployments()" class="bg-slate-700 hover:bg-slate-600 text-slate-300 font-medium py-2 px-5 text-sm rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-slate-500 focus:ring-offset-2 focus:ring-offset-slate-900">
        Reset Saved
      </button>
    </div>
  </div>
</div>

<!-- Deploy cards container -->
<div id="deploys-container" class="space-y-6"></div>

</div><!-- /deploy-view -->

<!-- ═══ Deployments view ═══ -->
<div id="deployments-view" class="hidden">
  <div class="bg-slate-900 border border-slate-800 rounded-xl shadow-xl overflow-hidden">
    <div class="overflow-x-auto">
      <table class="w-full text-sm text-left">
        <thead class="bg-slate-800 text-slate-400 text-xs uppercase tracking-wider">
          <tr>
            <th class="px-4 py-3">Customer</th>
            <th class="px-4 py-3">Email</th>
            <th class="px-4 py-3">Provider</th>
            <th class="px-4 py-3">Hostname</th>
            <th class="px-4 py-3">IP</th>
            <th class="px-4 py-3">Region</th>
            <th class="px-4 py-3">Status</th>
            <th class="px-4 py-3">Created</th>
            <th class="px-4 py-3">Actions</th>
          </tr>
        </thead>
        <tbody id="deployments-tbody" class="divide-y divide-slate-800"></tbody>
      </table>
    </div>
    <div id="deployments-empty" class="hidden px-4 py-8 text-center text-slate-500 text-sm">No deployments found.</div>
  </div>
  <!-- Pagination -->
  <div id="deployments-pagination" class="flex items-center justify-center gap-4 mt-4 text-sm">
    <button onclick="deploymentsPage(-1)" id="dep-prev" class="px-3 py-1.5 bg-slate-800 hover:bg-slate-700 text-slate-300 rounded-lg disabled:opacity-40 disabled:cursor-not-allowed" disabled>Prev</button>
    <span id="dep-page-info" class="text-slate-400">Page 1 of 1</span>
    <button onclick="deploymentsPage(1)" id="dep-next" class="px-3 py-1.5 bg-slate-800 hover:bg-slate-700 text-slate-300 rounded-lg disabled:opacity-40 disabled:cursor-not-allowed" disabled>Next</button>
  </div>
</div><!-- /deployments-view -->

</main>

<script>
const TOTAL_STEPS = 16;
const DEPLOY_STORAGE_KEY = 'clawmacdo.savedDeploys.v1';
let deployCounter = 0;
let backupOptions = '<option value="none">None</option>';

const eyeClosed = '<svg class="w-5 h-5 eye-closed" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M3.98 8.223A10.477 10.477 0 001.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.45 10.45 0 0112 4.5c4.756 0 8.773 3.162 10.065 7.498a10.523 10.523 0 01-4.293 5.774M6.228 6.228L3 3m3.228 3.228l3.65 3.65m7.894 7.894L21 21m-3.228-3.228l-3.65-3.65m0 0a3 3 0 10-4.243-4.243m4.242 4.242L9.88 9.88"/></svg>';
const eyeOpen = '<svg class="w-5 h-5 eye-open hidden" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>';

function eyeBtn() {
  return `<button type="button" class="eye-btn" onclick="toggleEye(this)">${eyeClosed}${eyeOpen}</button>`;
}

const MODEL_DEFS = {
  anthropic: { keyField: 'anthropic_key', label: 'Anthropic Key / Setup Token', placeholder: 'sk-ant-api-... or sk-ant-oat-...' },
  openai:    { keyField: 'openai_key',    label: 'OpenAI API Key',              placeholder: 'sk-...' },
  gemini:    { keyField: 'gemini_key',    label: 'Gemini API Key',              placeholder: 'AI...' },
};
const ALL_MODELS = ['anthropic', 'openai', 'gemini'];

function syncModelSelectors(n) {
  const container = document.getElementById('model-selectors-' + n);
  if (!container) return;
  const card = document.getElementById('deploy-card-' + n);

  // Preserve current values
  const saved = {};
  container.querySelectorAll('select[data-model-slot]').forEach(sel => {
    saved[sel.dataset.modelSlot] = sel.value;
  });
  container.querySelectorAll('input[data-model-key]').forEach(inp => {
    saved[inp.name] = inp.value;
  });

  // Determine selections
  const primary = saved.primary || 'anthropic';
  const fo1 = saved.failover_1 || '';
  const fo2 = saved.failover_2 || '';

  // Available for failover 1 = all models except primary
  const avail1 = ALL_MODELS.filter(m => m !== primary);
  // Available for failover 2 = avail1 minus fo1 (if fo1 is a model)
  const avail2 = avail1.filter(m => m !== fo1);

  // Build HTML
  let html = '';

  // Primary model
  html += '<div class="grid grid-cols-1 sm:grid-cols-2 gap-4">';
  html += '<div>';
  html += '<label class="block text-sm font-medium text-slate-300 mb-1">Primary Model <span class="text-red-400">*</span></label>';
  html += '<select data-model-slot="primary" onchange="syncModelSelectors(' + n + ')" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">';
  ALL_MODELS.forEach(m => {
    html += '<option value="' + m + '"' + (m === primary ? ' selected' : '') + '>' + capitalize(m) + '</option>';
  });
  html += '</select></div>';
  const pDef = MODEL_DEFS[primary];
  html += '<div data-field="' + pDef.keyField + '">';
  html += '<label class="block text-sm font-medium text-slate-300 mb-1">' + pDef.label + ' <span class="field-required-indicator text-red-400">*</span></label>';
  html += '<div class="relative"><input type="password" name="' + pDef.keyField + '" data-model-key="primary" data-required class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 pr-10 sm:pr-12 text-sm sm:text-base text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="' + pDef.placeholder + '" value="' + esc(saved[pDef.keyField] || '') + '">' + eyeBtn() + '</div></div>';
  html += '</div>';

  // Failover 1
  html += '<div class="grid grid-cols-1 sm:grid-cols-2 gap-4 mt-2">';
  html += '<div>';
  html += '<label class="block text-sm font-medium text-slate-300 mb-1">Failover 1 <span class="text-slate-500">(optional)</span></label>';
  html += '<select data-model-slot="failover_1" onchange="syncModelSelectors(' + n + ')" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">';
  html += '<option value=""' + (fo1 === '' ? ' selected' : '') + '>None</option>';
  avail1.forEach(m => {
    html += '<option value="' + m + '"' + (m === fo1 ? ' selected' : '') + '>' + capitalize(m) + '</option>';
  });
  html += '</select></div>';
  if (fo1 && MODEL_DEFS[fo1]) {
    const f1Def = MODEL_DEFS[fo1];
    html += '<div data-field="' + f1Def.keyField + '">';
    html += '<label class="block text-sm font-medium text-slate-300 mb-1">' + f1Def.label + ' <span class="field-required-indicator text-slate-500">(optional)</span></label>';
    html += '<div class="relative"><input type="password" name="' + f1Def.keyField + '" data-model-key="failover_1" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 pr-10 sm:pr-12 text-sm sm:text-base text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="' + f1Def.placeholder + '" value="' + esc(saved[f1Def.keyField] || '') + '">' + eyeBtn() + '</div></div>';
  }
  html += '</div>';

  // Failover 2 (only if failover 1 is selected)
  if (fo1) {
    html += '<div class="grid grid-cols-1 sm:grid-cols-2 gap-4 mt-2">';
    html += '<div>';
    html += '<label class="block text-sm font-medium text-slate-300 mb-1">Failover 2 <span class="text-slate-500">(optional)</span></label>';
    html += '<select data-model-slot="failover_2" onchange="syncModelSelectors(' + n + ')" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">';
    html += '<option value=""' + (fo2 === '' ? ' selected' : '') + '>None</option>';
    avail2.forEach(m => {
      html += '<option value="' + m + '"' + (m === fo2 ? ' selected' : '') + '>' + capitalize(m) + '</option>';
    });
    html += '</select></div>';
    if (fo2 && MODEL_DEFS[fo2]) {
      const f2Def = MODEL_DEFS[fo2];
      html += '<div data-field="' + f2Def.keyField + '">';
      html += '<label class="block text-sm font-medium text-slate-300 mb-1">' + f2Def.label + ' <span class="field-required-indicator text-slate-500">(optional)</span></label>';
      html += '<div class="relative"><input type="password" name="' + f2Def.keyField + '" data-model-key="failover_2" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 pr-10 sm:pr-12 text-sm sm:text-base text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="' + f2Def.placeholder + '" value="' + esc(saved[f2Def.keyField] || '') + '">' + eyeBtn() + '</div></div>';
    }
    html += '</div>';
  }

  container.innerHTML = html;
}

function capitalize(s) { return s.charAt(0).toUpperCase() + s.slice(1); }

function passwordField(name, label, placeholder, required) {
  const req = required
    ? '<span class="field-required-indicator text-red-400">*</span>'
    : '<span class="field-required-indicator text-slate-500">(optional)</span>';
  const reqAttr = required ? 'data-required' : '';
  return `<div data-field="${name}">
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

function loadSavedDeployments() {
  try {
    const raw = localStorage.getItem(DEPLOY_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(d => d && d.ip && d.keyPath && d.hostname);
  } catch (_) {
    return [];
  }
}

function saveSavedDeployments(items) {
  localStorage.setItem(DEPLOY_STORAGE_KEY, JSON.stringify(items));
}

function deployFingerprint(ip, keyPath, hostname) {
  return `${ip}|${keyPath}|${hostname}`;
}

function persistCompletedDeployment(ip, keyPath, hostname) {
  const list = loadSavedDeployments();
  const fp = deployFingerprint(ip, keyPath, hostname);
  const next = list.filter(d => deployFingerprint(d.ip, d.keyPath, d.hostname) !== fp);
  next.push({ ip, keyPath, hostname, savedAt: Date.now() });
  saveSavedDeployments(next);
}

function removeCompletedDeployment(fp) {
  if (!fp) return;
  const list = loadSavedDeployments();
  const next = list.filter(d => deployFingerprint(d.ip, d.keyPath, d.hostname) !== fp);
  saveSavedDeployments(next);
}

function resetSavedDeployments() {
  localStorage.removeItem(DEPLOY_STORAGE_KEY);
  document.querySelectorAll('[id^="deploy-card-"][data-deploy-fingerprint]').forEach(c => c.remove());
  if (!document.querySelector('[id^="deploy-card-"]')) {
    addDeployCard();
  }
  alert('Saved completed deployments were cleared.');
}

function restoreSavedDeployments() {
  const saved = loadSavedDeployments();
  for (const d of saved) {
    addDeployCard({ completed: true, ip: d.ip, keyPath: d.keyPath, hostname: d.hostname, restored: true });
  }
}

// ── Add deploy card ─────────────────────────────────────────────────────

function addDeployCard(initialState) {
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
    <form class="space-y-6" novalidate onsubmit="startDeploy(event, ${n})">
      <fieldset class="space-y-4">
        <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Customer Information</legend>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div data-field="customer_name">
            <label class="block text-sm font-medium text-slate-300 mb-1">Customer Name <span class="field-required-indicator text-red-400">*</span></label>
            <input type="text" name="customer_name" data-required placeholder="Jane Doe" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent placeholder-slate-500" />
          </div>
          <div data-field="customer_email">
            <label class="block text-sm font-medium text-slate-300 mb-1">Customer Email <span class="field-required-indicator text-red-400">*</span></label>
            <input type="text" name="customer_email" data-required data-validate-email placeholder="jane@example.com" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent placeholder-slate-500" />
          </div>
        </div>
      </fieldset>
      <fieldset class="space-y-4">
        <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">Cloud Provider</legend>
        <div>
          <label class="block text-sm font-medium text-slate-300 mb-1">Provider</label>
          <select name="provider" onchange="toggleProvider(this, ${n})" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
            <option value="digitalocean" selected>DigitalOcean</option>
            <option value="tencent">Tencent Cloud</option>
            <option value="lightsail">AWS Lightsail</option>
            <option value="azure">Microsoft Azure</option>
          </select>
        </div>
        <div id="do-creds-${n}">
          ${passwordField('do_token', 'DigitalOcean Token', 'dop_v1_...', true)}
        </div>
        <div id="tc-creds-${n}" style="display:none" class="space-y-4">
          ${passwordField('tencent_secret_id', 'Tencent SecretId', 'AKIDxxxxxxxx', true)}
          ${passwordField('tencent_secret_key', 'Tencent SecretKey', 'xxxxxxxx', true)}
        </div>
        <div id="aws-creds-${n}" style="display:none" class="space-y-4">
          ${passwordField('aws_access_key_id', 'AWS Access Key ID', 'AKIA...', true)}
          ${passwordField('aws_secret_access_key', 'AWS Secret Access Key', '...', true)}
        </div>
        <div id="azure-creds-${n}" style="display:none" class="space-y-4">
          ${passwordField('azure_tenant_id', 'Azure Tenant ID', 'xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx', true)}
          ${passwordField('azure_subscription_id', 'Azure Subscription ID', 'xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx', true)}
          ${passwordField('azure_client_id', 'Azure Client ID', 'xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx', true)}
          ${passwordField('azure_client_secret', 'Azure Client Secret', '...', true)}
        </div>
      </fieldset>
      <fieldset class="space-y-4">
        <legend class="text-sm font-medium text-slate-400 uppercase tracking-wider mb-2">AI Models</legend>
        <div id="model-selectors-${n}" class="space-y-4"></div>
        <div>
          <label class="block text-sm font-medium text-slate-300 mb-1">Tools Profile</label>
          <select name="profile" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
            <option value="full" selected>Full (unrestricted access to all tools)</option>
            <option value="coding">Coding (code-focused tools only)</option>
            <option value="messaging">Messaging (messaging tools only)</option>
          </select>
          <p class="text-xs text-slate-500 mt-1">Controls which tool groups the OpenClaw agent can access</p>
        </div>
        ${passwordField('tailscale_auth_key', 'Tailscale Auth Key', 'tskey-auth-...', false)}
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
            <select name="region" id="region-${n}" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
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
            <label class="block text-sm font-medium text-slate-300 mb-1">Instance Size</label>
            <select name="size" id="size-${n}" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 text-sm sm:text-base text-slate-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent">
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
  syncModelSelectors(n);
  const form = card.querySelector('form');
  const tailscaleToggle = form.querySelector('[name="tailscale"]');
  if (tailscaleToggle) {
    tailscaleToggle.addEventListener('change', () => syncTailscaleKeyRequirement(form));
    syncTailscaleKeyRequirement(form);
  }
  if (initialState && initialState.completed) {
    const progressDiv = card.querySelector('.deploy-progress');
    progressDiv.classList.remove('hidden');
    panelSetStatus(card, 'completed');
    panelUpdateProgress(card, TOTAL_STEPS);
    panelShowSummary(card, initialState.ip, initialState.keyPath, initialState.hostname);
    if (initialState.restored) {
      panelAppendLog(card, 'Recovered completed deployment from local storage.', 'text-slate-400');
    }
  }
  if (!initialState || !initialState.restored) {
    card.scrollIntoView({ behavior: 'smooth' });
  }
}

function validateForm(form) {
  // Clear previous errors
  form.querySelectorAll('.field-error').forEach(el => el.remove());
  form.querySelectorAll('.border-red-500').forEach(el => {
    el.classList.remove('border-red-500');
    el.classList.add('border-slate-700');
  });

  let valid = true;
  let firstError = null;

  // Check all fields with data-required that are visible
  form.querySelectorAll('[data-required]').forEach(input => {
    const wrapper = input.closest('[data-field]') || input.closest('div');
    // Skip hidden fields (e.g. provider credentials toggled off)
    if (input.offsetParent === null) return;

    const value = input.value.trim();
    let errorMsg = null;

    if (!value) {
      const label = wrapper.querySelector('label');
      let fieldName = input.name;
      if (label) {
        const clone = label.cloneNode(true);
        clone.querySelectorAll('.field-required-indicator').forEach(s => s.remove());
        fieldName = clone.textContent.trim();
      }
      errorMsg = fieldName + ' is required';
    } else if (input.hasAttribute('data-validate-email') && value) {
      if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)) {
        errorMsg = 'Please enter a valid email address';
      }
    }

    if (errorMsg) {
      valid = false;
      input.classList.remove('border-slate-700');
      input.classList.add('border-red-500');
      const err = document.createElement('p');
      err.className = 'field-error text-red-400 text-xs mt-1';
      err.textContent = errorMsg;
      input.closest('.relative')?.appendChild(err) || input.parentNode.appendChild(err);
      if (!firstError) firstError = input;
    }
  });

  if (firstError) firstError.focus();
  return valid;
}

// Clear error styling on input
document.addEventListener('input', function(e) {
  const input = e.target;
  if (input.classList.contains('border-red-500')) {
    input.classList.remove('border-red-500');
    input.classList.add('border-slate-700');
    const err = (input.closest('.relative') || input.parentNode).querySelector('.field-error');
    if (err) err.remove();
  }
});

function syncTailscaleKeyRequirement(form) {
  const tailscaleToggle = form.querySelector('[name="tailscale"]');
  const tailscaleKeyInput = form.querySelector('[name="tailscale_auth_key"]');
  const indicator = form.querySelector('[data-field="tailscale_auth_key"] .field-required-indicator');
  if (!tailscaleToggle || !tailscaleKeyInput) return;

  const isRequired = tailscaleToggle.checked;
  if (isRequired) {
    tailscaleKeyInput.setAttribute('data-required', '');
  } else {
    tailscaleKeyInput.removeAttribute('data-required');
  }

  if (indicator) {
    if (isRequired) {
      indicator.className = 'field-required-indicator text-red-400';
      indicator.textContent = '* required when Tailscale enabled';
    } else {
      indicator.className = 'field-required-indicator text-slate-500';
      indicator.textContent = '(optional)';
    }
  }
}

function toggleProvider(select, n) {
  const provider = select.value;
  const doCreds = document.getElementById('do-creds-' + n);
  const tcCreds = document.getElementById('tc-creds-' + n);
  const awsCreds = document.getElementById('aws-creds-' + n);
  const azureCreds = document.getElementById('azure-creds-' + n);
  const regionSel = document.getElementById('region-' + n);
  const sizeSel = document.getElementById('size-' + n);

  // Hide all credential sections and clear required
  doCreds.style.display = 'none';
  tcCreds.style.display = 'none';
  awsCreds.style.display = 'none';
  azureCreds.style.display = 'none';
  doCreds.querySelectorAll('input').forEach(i => { i.required = false; i.value = ''; });
  tcCreds.querySelectorAll('input').forEach(i => { i.required = false; i.value = ''; });
  awsCreds.querySelectorAll('input').forEach(i => { i.required = false; i.value = ''; });
  azureCreds.querySelectorAll('input').forEach(i => { i.required = false; i.value = ''; });

  if (provider === 'tencent') {
    tcCreds.style.display = 'block';
    tcCreds.querySelectorAll('input').forEach(i => i.required = true);
    regionSel.innerHTML = `
      <option value="ap-singapore" selected>ap-singapore (Singapore)</option>
      <option value="ap-hongkong">ap-hongkong (Hong Kong)</option>
      <option value="ap-guangzhou">ap-guangzhou (Guangzhou)</option>
      <option value="ap-shanghai">ap-shanghai (Shanghai)</option>
      <option value="ap-beijing">ap-beijing (Beijing)</option>
      <option value="ap-tokyo">ap-tokyo (Tokyo)</option>
      <option value="ap-seoul">ap-seoul (Seoul)</option>
      <option value="ap-mumbai">ap-mumbai (Mumbai)</option>
      <option value="eu-frankfurt">eu-frankfurt (Frankfurt)</option>
      <option value="na-siliconvalley">na-siliconvalley (Silicon Valley)</option>
    `;
    sizeSel.innerHTML = `
      <option value="SA5.MEDIUM2">SA5.MEDIUM2 (2 vCPU, 2 GB)</option>
      <option value="SA5.MEDIUM4" selected>SA5.MEDIUM4 (2 vCPU, 4 GB)</option>
      <option value="S8.MEDIUM4">S8.MEDIUM4 (2 vCPU, 4 GB)</option>
      <option value="SA5.MEDIUM8">SA5.MEDIUM8 (2 vCPU, 8 GB)</option>
      <option value="S8.LARGE8">S8.LARGE8 (4 vCPU, 8 GB)</option>
      <option value="SA5.LARGE16">SA5.LARGE16 (4 vCPU, 16 GB)</option>
    `;
  } else if (provider === 'lightsail') {
    awsCreds.style.display = 'block';
    awsCreds.querySelectorAll('input').forEach(i => i.required = true);
    regionSel.innerHTML = `
      <option value="ap-southeast-1" selected>ap-southeast-1 (Singapore)</option>
      <option value="us-east-1">us-east-1 (N. Virginia)</option>
      <option value="us-east-2">us-east-2 (Ohio)</option>
      <option value="us-west-2">us-west-2 (Oregon)</option>
      <option value="eu-west-1">eu-west-1 (Ireland)</option>
      <option value="eu-west-2">eu-west-2 (London)</option>
      <option value="eu-central-1">eu-central-1 (Frankfurt)</option>
      <option value="ap-northeast-1">ap-northeast-1 (Tokyo)</option>
      <option value="ap-northeast-2">ap-northeast-2 (Seoul)</option>
      <option value="ap-south-1">ap-south-1 (Mumbai)</option>
      <option value="ap-southeast-2">ap-southeast-2 (Sydney)</option>
      <option value="ca-central-1">ca-central-1 (Canada)</option>
    `;
    sizeSel.innerHTML = `
      <option value="micro">micro (1 vCPU, 1 GB - $10/mo)</option>
      <option value="small">small (1 vCPU, 2 GB - $15/mo)</option>
      <option value="medium" selected>medium (2 vCPUs, 4 GB - $30/mo)</option>
      <option value="large">large (2 vCPUs, 8 GB - $60/mo)</option>
      <option value="xlarge">xlarge (4 vCPUs, 16 GB - $120/mo)</option>
    `;
  } else if (provider === 'azure') {
    azureCreds.style.display = 'block';
    azureCreds.querySelectorAll('input').forEach(i => i.required = true);
    regionSel.innerHTML = `
      <option value="southeastasia" selected>southeastasia (Singapore)</option>
      <option value="eastasia">eastasia (Hong Kong)</option>
      <option value="eastus">eastus (East US)</option>
      <option value="eastus2">eastus2 (East US 2)</option>
      <option value="westus2">westus2 (West US 2)</option>
      <option value="westeurope">westeurope (Netherlands)</option>
      <option value="northeurope">northeurope (Ireland)</option>
      <option value="uksouth">uksouth (UK South)</option>
      <option value="japaneast">japaneast (Tokyo)</option>
      <option value="koreacentral">koreacentral (Seoul)</option>
      <option value="australiaeast">australiaeast (Sydney)</option>
      <option value="centralindia">centralindia (Pune)</option>
      <option value="canadacentral">canadacentral (Toronto)</option>
    `;
    sizeSel.innerHTML = `
      <option value="Standard_B1ms">Standard_B1ms (1 vCPU, 2 GB)</option>
      <option value="Standard_B2s" selected>Standard_B2s (2 vCPUs, 4 GB)</option>
      <option value="Standard_B2ms">Standard_B2ms (2 vCPUs, 8 GB)</option>
      <option value="Standard_B4ms">Standard_B4ms (4 vCPUs, 16 GB)</option>
      <option value="Standard_D2s_v5">Standard_D2s_v5 (2 vCPUs, 8 GB)</option>
      <option value="Standard_D4s_v5">Standard_D4s_v5 (4 vCPUs, 16 GB)</option>
    `;
  } else {
    doCreds.style.display = 'block';
    doCreds.querySelectorAll('input').forEach(i => i.required = true);
    regionSel.innerHTML = `
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
    `;
    sizeSel.innerHTML = `
      <option value="s-1vcpu-1gb">s-1vcpu-1gb (1 vCPU, 1 GB - $6/mo)</option>
      <option value="s-1vcpu-2gb">s-1vcpu-2gb (1 vCPU, 2 GB - $12/mo)</option>
      <option value="s-2vcpu-4gb" selected>s-2vcpu-4gb (2 vCPUs, 4 GB - $24/mo)</option>
      <option value="s-4vcpu-8gb">s-4vcpu-8gb (4 vCPUs, 8 GB - $48/mo)</option>
      <option value="s-8vcpu-16gb">s-8vcpu-16gb (8 vCPUs, 16 GB - $96/mo)</option>
    `;
  }
}

function removeCard(n) {
  const card = document.getElementById('deploy-card-' + n);
  if (card) {
    removeCompletedDeployment(card.dataset.deployFingerprint || '');
    card.remove();
  }
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
  panel.dataset.deployFingerprint = deployFingerprint(ip, keyPath, hostname);
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
      <p class="text-slate-300 font-semibold mb-2">Agent Docker Access</p>
      <p class="text-xs text-slate-400 mb-2">If bot replies with docker socket permission errors, run this fix.</p>
      <button type="button" onclick="fixAgentDockerAccess(this)" class="agent-docker-fix-btn bg-rose-600 hover:bg-rose-500 text-white font-semibold px-4 py-2 rounded-lg">Fix Agent Docker Access</button>
      <pre class="agent-docker-output mt-2 bg-slate-900 border border-slate-700 rounded-lg p-2 font-mono text-[10px] leading-3 whitespace-pre text-slate-300 min-h-[12rem] max-h-[40vh] overflow-auto"></pre>
    </div>
    <div class="mt-3 border border-slate-700 rounded-lg p-3">
      <p class="text-slate-300 font-semibold mb-2">WhatsApp Link QR</p>
      <p class="text-xs text-slate-400 mb-2">If unsupported, run Install/Repair first. Then click Fetch and scan from WhatsApp mobile (Linked Devices).</p>
      <div class="flex flex-col sm:flex-row gap-2">
        <button type="button" onclick="repairWhatsAppSupport(this)" class="whatsapp-repair-btn bg-amber-600 hover:bg-amber-500 text-white font-semibold px-4 py-2 rounded-lg">Install/Repair WhatsApp Support</button>
        <button type="button" onclick="fetchWhatsAppQr(this)" class="whatsapp-qr-btn bg-blue-600 hover:bg-blue-500 text-white font-semibold px-4 py-2 rounded-lg">Fetch QR</button>
      </div>
      <pre class="whatsapp-qr-output mt-2 bg-slate-900 border border-slate-700 rounded-lg p-2 font-mono text-[10px] leading-3 whitespace-pre text-slate-300 min-h-[26rem] max-h-[70vh] overflow-auto"></pre>
    </div>
  `;
  persistCompletedDeployment(ip, keyPath, hostname);
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
      const msg = data.message || 'Failed to fetch WhatsApp QR.';
      const details = (data.qr_output || '').trim();
      output.textContent = details ? (msg + '\n\n' + details) : msg;
    }
  } catch (err) {
    output.textContent = 'Request failed: ' + err.message;
  } finally {
    btn.disabled = false;
    btn.textContent = oldText;
  }
}

async function fixAgentDockerAccess(btn) {
  const panel = btn.closest('[id^="deploy-card-"]');
  if (!panel) return;

  const output = panel.querySelector('.agent-docker-output');
  const ip = panel.dataset.deployIp || '';
  const keyPath = panel.dataset.deployKeyPath || '';
  if (!ip || !keyPath) {
    output.textContent = 'Missing deploy connection details.';
    return;
  }

  const oldText = btn.textContent;
  btn.disabled = true;
  btn.textContent = 'Fixing...';
  output.textContent = 'Applying Docker access repair on droplet...';

  try {
    const res = await fetch('/api/agent/docker-fix', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ip: ip,
        ssh_key_path: keyPath
      })
    });
    const data = await res.json();
    const msg = data.message || 'Docker repair completed.';
    const details = (data.fix_output || '').trim();
    output.textContent = details ? (msg + '\\n\\n' + details) : msg;
  } catch (err) {
    output.textContent = 'Request failed: ' + err.message;
  } finally {
    btn.disabled = false;
    btn.textContent = oldText;
  }
}

async function repairWhatsAppSupport(btn) {
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
  btn.textContent = 'Repairing...';
  output.textContent = 'Updating OpenClaw and refreshing extensions on droplet...';

  try {
    const res = await fetch('/api/whatsapp/repair', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        ip: ip,
        ssh_key_path: keyPath
      })
    });
    const data = await res.json();
    const msg = data.message || 'WhatsApp repair completed.';
    const details = (data.repair_output || '').trim();
    output.textContent = details ? (msg + '\n\n' + details) : msg;
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
  syncTailscaleKeyRequirement(form);
  if (!validateForm(form)) return;

  const val = (name) => (form.querySelector(`[name="${name}"]`) || {}).value || '';
  const selVal = (slot) => {
    const sel = card.querySelector(`select[data-model-slot="${slot}"]`);
    return sel ? sel.value : '';
  };
  const body = {
    customer_name: val('customer_name'),
    customer_email: val('customer_email'),
    provider: val('provider'),
    do_token: val('do_token'),
    tencent_secret_id: val('tencent_secret_id'),
    tencent_secret_key: val('tencent_secret_key'),
    aws_access_key_id: val('aws_access_key_id'),
    aws_secret_access_key: val('aws_secret_access_key'),
    aws_region: val('region'),
    azure_tenant_id: val('azure_tenant_id'),
    azure_subscription_id: val('azure_subscription_id'),
    azure_client_id: val('azure_client_id'),
    azure_client_secret: val('azure_client_secret'),
    anthropic_key: val('anthropic_key'),
    openai_key: val('openai_key'),
    gemini_key: val('gemini_key'),
    primary_model: selVal('primary'),
    failover_1: selVal('failover_1'),
    failover_2: selVal('failover_2'),
    whatsapp_phone_number: val('whatsapp_phone_number'),
    telegram_bot_token: val('telegram_bot_token'),
    region: val('region'),
    size: val('size'),
    hostname: val('hostname'),
    backup: val('backup'),
    enable_backups: form.querySelector('[name="enable_backups"]').checked,
    enable_sandbox: form.querySelector('[name="enable_sandbox"]').checked,
    tailscale: form.querySelector('[name="tailscale"]').checked,
    tailscale_auth_key: val('tailscale_auth_key'),
    profile: val('profile'),
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
    if (!res.ok) {
      throw new Error((data && data.message) || 'Failed to start deploy');
    }
    const deployId = data.deploy_id;

    const evtSource = new EventSource(`/api/deploy/${deployId}/events`);
    evtSource.onmessage = function(event) {
      const msg = event.data;

      if (msg.startsWith('DEPLOY_COMPLETE_JSON:')) {
        let ip = '';
        let keyPath = '';
        let hostname = '';
        try {
          const payload = JSON.parse(msg.substring('DEPLOY_COMPLETE_JSON:'.length));
          ip = payload.ip || '';
          keyPath = payload.ssh_key_path || '';
          hostname = payload.hostname || '';
        } catch (_) {}
        if (!ip || !keyPath || !hostname) {
          panelSetStatus(card, 'failed');
          panelAppendLog(card, 'ERROR: Invalid deploy completion payload', 'text-red-400 font-semibold');
          evtSource.close();
          btn.disabled = false;
          btn.textContent = 'Retry Deploy';
          btn.className = btn.className.replace('bg-slate-700 cursor-not-allowed', 'bg-blue-600 hover:bg-blue-500');
          return;
        }
        panelSetStatus(card, 'completed');
        panelUpdateProgress(card, TOTAL_STEPS);
        panelAppendLog(card, 'Deploy completed successfully!', 'text-green-400 font-semibold');
        evtSource.close();
        panelShowSummary(card, ip, keyPath, hostname);
        return;
      }

      if (msg.startsWith('DEPLOY_COMPLETE:')) {
        const parts = msg.split(':');
        const ip = parts[1] || '';
        const hostname = parts[parts.length - 1] || '';
        const keyPath = parts.slice(2, parts.length - 1).join(':');
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

// ── Tab switching ───────────────────────────────────────────────────────
function switchTab(tab) {
  const deployView = document.getElementById('deploy-view');
  const deploymentsView = document.getElementById('deployments-view');
  const tabDeploy = document.getElementById('tab-deploy');
  const tabDeployments = document.getElementById('tab-deployments');
  if (tab === 'deployments') {
    deployView.classList.add('hidden');
    deploymentsView.classList.remove('hidden');
    tabDeploy.className = 'px-4 py-2 text-sm font-medium border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition-colors';
    tabDeployments.className = 'px-4 py-2 text-sm font-medium border-b-2 border-blue-500 text-blue-400 transition-colors';
    loadDeployments();
  } else {
    deployView.classList.remove('hidden');
    deploymentsView.classList.add('hidden');
    tabDeploy.className = 'px-4 py-2 text-sm font-medium border-b-2 border-blue-500 text-blue-400 transition-colors';
    tabDeployments.className = 'px-4 py-2 text-sm font-medium border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition-colors';
  }
}

// ── Deployments table ───────────────────────────────────────────────────
let depCurrentPage = 1;

function statusBadge(status) {
  const colors = {
    'completed': 'bg-green-500/20 text-green-300 border-green-500/30',
    'running':   'bg-blue-500/20 text-blue-300 border-blue-500/30',
    'failed':    'bg-red-500/20 text-red-300 border-red-500/30',
    'dry-run':   'bg-yellow-500/20 text-yellow-300 border-yellow-500/30',
  };
  const cls = colors[status] || 'bg-slate-500/20 text-slate-300 border-slate-500/30';
  return '<span class="inline-block px-2 py-0.5 text-xs font-medium rounded-full border ' + cls + '">' + (status || 'unknown') + '</span>';
}

async function loadDeployments(page) {
  if (page) depCurrentPage = page;
  try {
    const res = await fetch('/api/deployments?page=' + depCurrentPage);
    const data = await res.json();
    const tbody = document.getElementById('deployments-tbody');
    const empty = document.getElementById('deployments-empty');
    tbody.innerHTML = '';
    const pagination = document.getElementById('deployments-pagination');
    if (!data.deployments || data.deployments.length === 0) {
      empty.classList.remove('hidden');
      pagination.classList.add('hidden');
    } else {
      pagination.classList.remove('hidden');
      empty.classList.add('hidden');
      for (const d of data.deployments) {
        const tr = document.createElement('tr');
        tr.className = 'hover:bg-slate-800/50';
        tr.innerHTML =
          '<td class="px-4 py-3 text-slate-200">' + esc(d.customer_name) + '</td>' +
          '<td class="px-4 py-3 text-slate-400">' + esc(d.customer_email) + '</td>' +
          '<td class="px-4 py-3 text-slate-400">' + esc(d.provider || '-') + '</td>' +
          '<td class="px-4 py-3 text-slate-300 font-mono text-xs">' + esc(d.hostname || '-') + '</td>' +
          '<td class="px-4 py-3 text-slate-300 font-mono text-xs">' + esc(d.ip_address || '-') + '</td>' +
          '<td class="px-4 py-3 text-slate-400">' + esc(d.region || '-') + '</td>' +
          '<td class="px-4 py-3">' + statusBadge(d.status) + '</td>' +
          '<td class="px-4 py-3 text-slate-500 text-xs whitespace-nowrap">' + formatSGT(d.created_at) + '</td>' +
          '<td class="px-4 py-3"><button onclick="deleteDeployment(\'' + esc(d.id) + '\')" class="text-red-400 hover:text-red-300 text-xs font-medium px-2 py-1 rounded bg-red-500/10 hover:bg-red-500/20 border border-red-500/20 transition-colors">Delete</button></td>';
        tbody.appendChild(tr);
      }
    }
    // Pagination
    const totalPages = data.total_pages || 1;
    document.getElementById('dep-page-info').textContent = 'Page ' + depCurrentPage + ' of ' + totalPages;
    document.getElementById('dep-prev').disabled = depCurrentPage <= 1;
    document.getElementById('dep-next').disabled = depCurrentPage >= totalPages;
  } catch (e) {
    console.error('Failed to load deployments:', e);
  }
}

function deploymentsPage(delta) {
  depCurrentPage += delta;
  if (depCurrentPage < 1) depCurrentPage = 1;
  loadDeployments();
}

async function deleteDeployment(id) {
  if (!confirm('Delete this deployment record?')) return;
  try {
    await fetch('/api/deployments/' + encodeURIComponent(id), { method: 'DELETE' });
    loadDeployments();
  } catch (e) {
    alert('Failed to delete: ' + e.message);
  }
}

function esc(s) {
  if (!s) return '';
  const d = document.createElement('div');
  d.textContent = s;
  return d.innerHTML;
}

function formatSGT(dateStr) {
  if (!dateStr) return '-';
  try {
    const d = new Date(dateStr);
    if (isNaN(d)) return esc(dateStr);
    return d.toLocaleString('en-SG', { timeZone: 'Asia/Singapore', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
  } catch (e) { return esc(dateStr); }
}

// ── Check dry-run config ────────────────────────────────────────────────
async function checkDryRun() {
  try {
    const res = await fetch('/api/config');
    const data = await res.json();
    if (data.dry_run) {
      document.getElementById('dry-run-banner').classList.remove('hidden');
    }
  } catch (_) {}
}
checkDryRun();

// Auto-add the first deployment card
addDeployCard();
restoreSavedDeployments();
</script>
</body>
</html>
"##;
