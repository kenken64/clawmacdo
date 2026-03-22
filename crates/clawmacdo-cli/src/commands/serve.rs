use crate::commands::deploy::{self, DeployParams};
use crate::commands::docker_fix;
use crate::commands::whatsapp;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, Sse};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use chrono::TimeZone;
use clawmacdo_cloud::CloudProvider;
use clawmacdo_core::config;
use clawmacdo_db as db;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_async, ssh_as_openclaw_with_user_async, ssh_root_async,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

// ── Shared state ────────────────────────────────────────────────────────────

type Db = Arc<Mutex<rusqlite::Connection>>;
type Jobs = Arc<RwLock<HashMap<String, DeployJob>>>;
type RateLimiter = Arc<Mutex<HashMap<IpAddr, (u32, std::time::Instant)>>>;

#[derive(Clone)]
struct AppState {
    jobs: Jobs,
    db: Db,
    rate_limiter: RateLimiter,
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
    #[serde(default)]
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
    byteplus_access_key: String,
    #[serde(default)]
    byteplus_secret_key: String,
    #[serde(default)]
    byteplus_ark_api_key: String,
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
    #[serde(default)]
    spot: bool,
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

fn db_lock_error() -> Response {
  (
    StatusCode::INTERNAL_SERVER_ERROR,
    Json(ErrorResponse {
      message: "Database lock poisoned".into(),
    }),
  )
    .into_response()
}

fn lock_db(db: &Db) -> Result<MutexGuard<'_, rusqlite::Connection>, Response> {
  db.lock().map_err(|_| db_lock_error())
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

#[derive(Deserialize)]
struct FunnelToggleRequest {
    /// "on" or "off"
    action: String,
    /// Port to expose (default 18789)
    #[serde(default = "default_funnel_port")]
    port: u16,
}

fn default_funnel_port() -> u16 {
    18789
}

#[derive(Serialize)]
struct FunnelToggleResponse {
    ok: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    funnel_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway_token: Option<String>,
}

#[derive(Deserialize)]
struct DestroyDeploymentRequest {
    #[serde(default)]
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
    azure_resource_group: String,
    #[serde(default)]
    byteplus_access_key: String,
    #[serde(default)]
    byteplus_secret_key: String,
}

#[derive(Serialize)]
struct DestroyDeploymentResponse {
    ok: bool,
    message: String,
}

#[derive(Deserialize)]
struct SnapshotDeploymentRequest {
    snapshot_name: String,
    #[serde(default)]
    do_token: String,
    #[serde(default)]
    aws_region: String,
    #[serde(default)]
    byteplus_access_key: String,
    #[serde(default)]
    byteplus_secret_key: String,
    #[serde(default)]
    byteplus_region: String,
}

#[derive(Serialize)]
struct SnapshotDeploymentResponse {
    ok: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_id: Option<String>,
}

#[derive(Deserialize)]
struct RestoreSnapshotRequest {
    snapshot_name: String,
    provider: String,
    #[serde(default)]
    do_token: String,
    #[serde(default)]
    aws_region: String,
    #[serde(default)]
    byteplus_access_key: String,
    #[serde(default)]
    byteplus_secret_key: String,
    #[serde(default)]
    byteplus_region: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    spot: bool,
}

#[derive(Deserialize)]
struct ArkApiKeyRequest {
    #[serde(default)]
    access_key: String,
    #[serde(default)]
    secret_key: String,
    #[serde(default = "default_resource_type")]
    resource_type: String,
    #[serde(default)]
    resource_ids: Vec<String>,
    #[serde(default = "default_duration")]
    duration_seconds: u64,
}

fn default_resource_type() -> String {
    "endpoint".to_string()
}

fn default_duration() -> u64 {
    2_592_000 // 30 days
}

#[derive(Serialize)]
struct ArkApiKeyResponse {
    ok: bool,
    api_key: String,
    expires: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ArkListEndpointsRequest {
    #[serde(default)]
    access_key: String,
    #[serde(default)]
    secret_key: String,
}

#[derive(Serialize)]
struct ArkListEndpointsResponse {
    ok: bool,
    endpoints: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct ConfigResponse {
    dry_run: bool,
}

// ── Security: Rate limiting ─────────────────────────────────────────────────

const RATE_LIMIT_MAX: u32 = 60; // max requests per window
const RATE_LIMIT_WINDOW_SECS: u64 = 60; // 1-minute window

async fn rate_limit_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<IpAddr>().ok())
        .unwrap_or_else(|| "127.0.0.1".parse().unwrap());

    let now = std::time::Instant::now();
    {
        let mut limiter = state.rate_limiter.lock().unwrap_or_else(|e| e.into_inner());
        let entry = limiter.entry(ip).or_insert((0, now));
        if now.duration_since(entry.1).as_secs() >= RATE_LIMIT_WINDOW_SECS {
            *entry = (1, now);
        } else {
            entry.0 += 1;
            if entry.0 > RATE_LIMIT_MAX {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    "Rate limit exceeded. Try again later.",
                )
                    .into_response();
            }
        }
    }

    next.run(req).await
}

// ── Security: API key middleware ────────────────────────────────────────────

async fn api_key_middleware(req: Request<Body>, next: Next) -> Response {
    let api_key = std::env::var("CLAWMACDO_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        // No API key configured — allow all (dev mode)
        return next.run(req).await;
    }

    // Check X-API-Key header (for programmatic / curl access)
    if let Some(val) = req.headers().get("x-api-key") {
        if val.as_bytes() == api_key.as_bytes() {
            return next.run(req).await;
        }
    }

    // Also accept valid PIN session cookie (for browser-based web UI fetch calls)
    if let Some(pin) = get_configured_pin() {
        let expected_token = generate_session_token(&pin);
        let has_valid_session = req
            .headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .map(|cookies| {
                cookies.split(';').any(|c| {
                    let c = c.trim();
                    c == format!("{PIN_COOKIE_NAME}={expected_token}")
                })
            })
            .unwrap_or(false);
        if has_valid_session {
            return next.run(req).await;
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "Invalid or missing API key"})),
    )
        .into_response()
}

// ── Security: PIN-based web page auth ──────────────────────────────────────

const PIN_COOKIE_NAME: &str = "clawmacdo_session";

/// Returns the active 6-digit PIN, generating one automatically if
/// `CLAWMACDO_PIN` is not set or invalid.  The generated PIN is stable
/// for the lifetime of the process (stored in a `OnceLock`).
fn get_configured_pin() -> Option<String> {
    static PIN: OnceLock<String> = OnceLock::new();
    Some(
        PIN.get_or_init(|| {
            let env_pin = std::env::var("CLAWMACDO_PIN").unwrap_or_default();
            if env_pin.len() == 6 && env_pin.chars().all(|c| c.is_ascii_digit()) {
                return env_pin;
            }
            // Auto-generate a 6-digit PIN from system randomness
            use std::collections::hash_map::RandomState;
            use std::hash::{BuildHasher, Hasher};
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u64(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            );
            let n = hasher.finish() % 1_000_000;
            format!("{n:06}")
        })
        .clone(),
    )
}

fn generate_session_token(pin: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    pin.hash(&mut hasher);
    "sess_".to_string() + &format!("{:x}", hasher.finish())
}

async fn pin_auth_middleware(req: Request<Body>, next: Next) -> Response {
    let pin = match get_configured_pin() {
        Some(p) => p,
        None => return next.run(req).await, // No PIN configured — allow all
    };

    let path = req.uri().path().to_string();

    // Allow login page and static assets without auth
    if path == "/login" || path.starts_with("/assets/") {
        return next.run(req).await;
    }

    // Check session cookie
    let expected_token = generate_session_token(&pin);
    let has_valid_session = req
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|cookies| {
            cookies.split(';').any(|c| {
                let c = c.trim();
                c == format!("{PIN_COOKIE_NAME}={expected_token}")
            })
        })
        .unwrap_or(false);

    if has_valid_session {
        return next.run(req).await;
    }

    // Redirect to login
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/login")
        .body(Body::empty())
        .unwrap()
}

async fn login_page_handler() -> Html<String> {
    Html(LOGIN_HTML.to_string())
}

#[derive(Deserialize)]
struct LoginForm {
    pin: String,
}

async fn login_submit_handler(
    axum::extract::Form(form): axum::extract::Form<LoginForm>,
) -> Response {
    let pin = match get_configured_pin() {
        Some(p) => p,
        None => {
            return Response::builder()
                .status(StatusCode::FOUND)
                .header(header::LOCATION, "/")
                .body(Body::empty())
                .unwrap();
        }
    };

    if form.pin == pin {
        let token = generate_session_token(&pin);
        Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, "/")
            .header(
                header::SET_COOKIE,
                format!(
                    "{PIN_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=86400"
                ),
            )
            .body(Body::empty())
            .unwrap()
    } else {
        Html(LOGIN_HTML.replace(
            "<!-- ERROR -->",
            r#"<p style="color:#ef4444;margin-bottom:1rem;font-size:0.875rem">Invalid PIN. Please try again.</p>"#,
        ))
        .into_response()
    }
}

async fn logout_handler() -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/login")
        .header(
            header::SET_COOKIE,
            format!("{PIN_COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"),
        )
        .body(Body::empty())
        .unwrap()
}

// ── Entry point ─────────────────────────────────────────────────────────────

/// Run the web server.
pub async fn run(port: u16) -> anyhow::Result<()> {
    let conn = db::init_db()?;
    let db: Db = Arc::new(Mutex::new(conn));
    let jobs: Jobs = Arc::new(RwLock::new(HashMap::new()));
    let rate_limiter: RateLimiter = Arc::new(Mutex::new(HashMap::new()));
    let state = AppState {
        jobs,
        db,
        rate_limiter,
    };

    // CORS — restrict to same-origin; allow standard methods and headers
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(
            format!("http://localhost:{port}").parse().unwrap(),
        ))
        .allow_methods(AllowMethods::list([
            Method::GET,
            Method::POST,
            Method::DELETE,
        ]))
        .allow_headers(AllowHeaders::list([
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-api-key"),
        ]));

    // API routes — protected by API key middleware
    let api_routes = Router::new()
        .route("/api/backups", get(list_backups_handler))
        .route("/api/deploy", post(start_deploy_handler))
        .route("/api/deploy/{id}/events", get(deploy_events_handler))
        .route("/api/deploy/steps/{id}", get(deploy_steps_handler))
        .route(
            "/api/telegram/pairing/approve",
            post(approve_telegram_pairing_handler),
        )
        .route("/api/agent/docker-fix", post(repair_agent_docker_handler))
        .route("/api/whatsapp/repair", post(repair_whatsapp_handler))
        .route("/api/whatsapp/qr", post(fetch_whatsapp_qr_handler))
        .route("/api/deployments", get(list_deployments_handler))
        .route("/api/deployments/{id}", delete(delete_deployment_handler))
        .route(
            "/api/deployments/{id}/destroy",
            post(destroy_deployment_handler),
        )
        .route(
            "/api/deployments/{id}/snapshot",
            post(snapshot_deployment_handler),
        )
        .route("/api/deployments/{id}/refresh-ip", post(refresh_ip_handler))
        .route("/api/deployments/{id}/funnel", post(toggle_funnel_handler))
        .route(
            "/api/deployments/{id}/funnel/status",
            get(funnel_status_handler),
        )
        .route(
            "/api/deployments/{id}/whatsapp/repair",
            post(deployment_whatsapp_repair_handler),
        )
        .route(
            "/api/deployments/{id}/whatsapp/qr",
            post(deployment_whatsapp_qr_handler),
        )
        .route(
            "/api/deployments/{id}/devices/approve",
            post(device_approve_handler),
        )
        .route("/api/snapshots/restore", post(restore_snapshot_handler))
        .route("/api/snapshots", get(list_snapshots_handler))
        .route("/api/config", get(config_handler))
        .route("/api/ark/endpoints", post(ark_list_endpoints_handler))
        .route("/api/ark/api-key", post(ark_api_key_handler))
        .layer(middleware::from_fn(api_key_middleware));

    // Web routes — protected by PIN auth middleware
    let web_routes = Router::new()
        .route("/", get(index_handler))
        .route("/assets/mascot.jpg", get(mascot_handler))
        .layer(middleware::from_fn(pin_auth_middleware));

    // Login/logout routes — always accessible
    let login_routes = Router::new()
        .route("/login", get(login_page_handler))
        .route("/login", post(login_submit_handler))
        .route("/logout", get(logout_handler));

    let app = Router::new()
        .merge(login_routes)
        .merge(web_routes)
        .merge(api_routes)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .layer(cors)
        .with_state(state);

    let bind_addr = std::env::var("CLAWMACDO_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let addr = format!("{bind_addr}:{port}");
    println!("ClawMacToDO web UI running at http://{addr}");
    if bind_addr == "127.0.0.1" {
        println!("  (localhost only — set CLAWMACDO_BIND=0.0.0.0 to allow remote access)");
    }
    if let Some(pin) = get_configured_pin() {
        let source = if std::env::var("CLAWMACDO_PIN")
            .map(|v| v.len() == 6 && v.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false)
        {
            "from CLAWMACDO_PIN"
        } else {
            "auto-generated"
        };
        println!("  PIN: {pin}  ({source})");
    }
    if !std::env::var("CLAWMACDO_API_KEY")
        .unwrap_or_default()
        .is_empty()
    {
        println!("  API key protection enabled (CLAWMACDO_API_KEY)");
    }
    println!("Press Ctrl+C to stop.\n");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ── Route handlers ──────────────────────────────────────────────────────────

/// Index handler.
async fn index_handler() -> Html<String> {
    Html(INDEX_HTML.replace("{version}", env!("CARGO_PKG_VERSION")))
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
        "byteplus" => !req.byteplus_ark_api_key.trim().is_empty(),
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

    let hostname = match config::normalize_hostname(&req.hostname) {
      Ok(value) => value,
      Err(err) => {
        return (
          StatusCode::BAD_REQUEST,
          Json(ErrorResponse {
            message: err.to_string(),
          }),
        )
          .into_response();
      }
    };

    let backup: Option<PathBuf> = if req.backup.is_empty() || req.backup == "none" {
      None
    } else {
      match config::resolve_backup_path(&req.backup) {
        Ok(path) => Some(path),
        Err(err) => {
          return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
              message: err.to_string(),
            }),
          )
            .into_response();
        }
      }
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

    // Insert deployment record BEFORE spawning so it's immediately visible
    // in the Deployments tab even if the user navigates away instantly.
    {
        let provider_str = &req.provider;
        let region_str = &req.region;
        let size_str = &req.size;
        let hostname_str = hostname.as_deref().unwrap_or("");
        if let Ok(conn) = db.lock() {
            let _ = db::insert_deployment(
                &conn,
                &id,
                &customer_name,
                &customer_email,
                provider_str,
                region_str,
                size_str,
                hostname_str,
            );
        }
    }

    tokio::spawn(async move {
        let params = DeployParams {
            deploy_id: Some(id.clone()),
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
            byteplus_access_key: req.byteplus_access_key,
            byteplus_secret_key: req.byteplus_secret_key,
            byteplus_ark_api_key: req.byteplus_ark_api_key,
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
            hostname,
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
            spot: req.spot,
            non_interactive: true,
            progress_tx: Some(tx.clone()),
            db: Some(db_clone.clone()),
        };

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
            let dry_hostname = if params.hostname.as_deref().unwrap_or("").is_empty() {
                format!("openclaw-{}", &id[..8])
            } else {
                params.hostname.clone().unwrap_or_default()
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

/// Return deploy/operation steps from SQLite for progress polling.
async fn deploy_steps_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
  let conn = match lock_db(&state.db) {
    Ok(conn) => conn,
    Err(resp) => return resp,
  };
    match db::get_deploy_steps(&conn, &id) {
    Ok(steps) => (StatusCode::OK, Json(serde_json::json!({ "steps": steps }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{e}") })),
    )
      .into_response(),
    }
}

/// Refresh the IP address of a deployment by querying the cloud provider.
async fn refresh_ip_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let (provider, hostname, region, old_ip) = {
    let conn = match lock_db(&state.db) {
      Ok(conn) => conn,
      Err(resp) => return resp,
    };
        match db::get_deployment_by_id(&conn, &id) {
            Ok(Some(d)) => (
                d.provider.unwrap_or_default(),
                d.hostname.unwrap_or_default(),
                d.region.unwrap_or_default(),
                d.ip_address.unwrap_or_default(),
            ),
            Ok(None) => {
              return Json(serde_json::json!({ "ok": false, "message": "Deployment not found." })).into_response()
            }
            Err(e) => {
              return Json(
                    serde_json::json!({ "ok": false, "message": format!("DB error: {e}") }),
                )
              .into_response()
            }
        }
    };

    if hostname.is_empty() {
        return Json(
            serde_json::json!({ "ok": false, "message": "No hostname in deploy record." }),
          )
          .into_response();
    }

    let new_ip = match provider.as_str() {
        "lightsail" => {
            let ls_region = if region.is_empty() {
                "ap-southeast-1".to_string()
            } else {
                region
            };
            let ls = clawmacdo_cloud::lightsail_cli::LightsailCliProvider::new(ls_region);
            match ls.wait_for_active(&hostname, 5).await {
                Ok(info) => match info.public_ip {
                    Some(ip) => ip,
                    None => {
                        return Json(serde_json::json!({
                            "ok": false, "message": "Instance has no public IP."
                        }))
                      .into_response()
                    }
                },
                Err(e) => {
                    return Json(serde_json::json!({
                        "ok": false, "message": format!("Failed to query instance: {e}")
                    }))
                    .into_response()
                }
            }
        }
        "digitalocean" => {
            let token = std::env::var("DO_TOKEN").unwrap_or_default();
            if token.is_empty() {
                return Json(serde_json::json!({
                    "ok": false, "message": "DO_TOKEN env var required."
              }))
              .into_response();
            }
            let client = match clawmacdo_cloud::digitalocean::DoClient::new(&token) {
                Ok(c) => c,
                Err(e) => {
                    return Json(serde_json::json!({
                        "ok": false, "message": format!("Invalid DO token: {e}")
                    }))
                .into_response()
                }
            };
            match client.list_droplets().await {
                Ok(droplets) => match droplets.iter().find(|d| d.name == hostname) {
                    Some(d) => match d.public_ip() {
                        Some(ip) => ip,
                        None => {
                            return Json(serde_json::json!({
                                "ok": false, "message": "Droplet has no public IP."
                            }))
                          .into_response()
                        }
                    },
                    None => {
                        return Json(serde_json::json!({
                            "ok": false, "message": format!("Droplet '{hostname}' not found.")
                        }))
                        .into_response()
                    }
                },
                Err(e) => {
                    return Json(serde_json::json!({
                        "ok": false, "message": format!("Failed to list droplets: {e}")
                    }))
                      .into_response()
                }
            }
        }
        other => {
            return Json(serde_json::json!({
                "ok": false, "message": format!("Refresh IP not supported for provider '{other}'.")
            }))
                  .into_response()
        }
    };

    if new_ip == old_ip {
                return Json(serde_json::json!({ "ok": true, "message": "IP unchanged.", "ip": new_ip })).into_response();
    }

    // Update SQLite
    {
        let conn = match lock_db(&state.db) {
            Ok(conn) => conn,
            Err(resp) => return resp,
        };
        let _ =
            db::update_deployment_status(&conn, &id, "completed", Some(&new_ip), Some(&hostname));
    }

    // Update JSON deploy record
    if let Ok(deploys_dir) = config::deploys_dir() {
        let path = deploys_dir.join(format!("{id}.json"));
        if path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(mut raw) = serde_json::from_str::<serde_json::Value>(&contents) {
                    raw["ip_address"] = serde_json::Value::String(new_ip.clone());
                    let _ = std::fs::write(
                        &path,
                        serde_json::to_string_pretty(&raw).unwrap_or_default(),
                    );
                }
            }
        }
    }

    Json(serde_json::json!({
        "ok": true,
        "message": format!("IP updated: {old_ip} -> {new_ip}"),
        "ip": new_ip,
        "old_ip": old_ip,
    }))
    .into_response()
}

/// Approve telegram pairing handler.
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

    let key = match config::resolve_key_path(&key_path) {
      Ok(path) => path,
      Err(err) => {
        return (
          StatusCode::BAD_REQUEST,
          Json(TelegramPairingApproveResponse {
            ok: false,
            message: err.to_string(),
          }),
        )
      }
    };
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

/// Extract the last QR code block from output that may contain multiple QR codes.
/// QR codes use Unicode block characters (█ ▀ ▄ ▐ ▌ etc).
fn extract_last_qr_block(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let is_qr_line = |line: &str| {
        line.contains('█')
            || line.contains('▀')
            || line.contains('▄')
            || line.contains('▐')
            || line.contains('▌')
            || line.contains('▖')
            || line.contains('▗')
            || line.contains('▘')
            || line.contains('▙')
            || line.contains('▚')
            || line.contains('▛')
            || line.contains('▜')
            || line.contains('▝')
            || line.contains('▞')
            || line.contains('▟')
    };

    // Find the last contiguous block of QR lines
    let mut last_end = None;
    let mut last_start = None;
    let mut i = lines.len();
    while i > 0 {
        i -= 1;
        if is_qr_line(lines[i]) {
            if last_end.is_none() {
                last_end = Some(i);
            }
            last_start = Some(i);
        } else if last_end.is_some() {
            break;
        }
    }

    match (last_start, last_end) {
        (Some(start), Some(end)) => lines[start..=end].join("\n"),
        _ => output.to_string(), // No QR found, return full output
    }
}

/// Fetch whatsapp qr handler.
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

    let key = match config::resolve_key_path(&key_path) {
      Ok(path) => path,
      Err(err) => {
        return (
          StatusCode::BAD_REQUEST,
          Json(WhatsAppQrResponse {
            ok: false,
            message: err.to_string(),
            qr_output: String::new(),
          }),
        )
      }
    };
    // Use 45s timeout — enough to capture the first QR code
    let cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if command -v timeout >/dev/null 2>&1; then \
           timeout 45s openclaw channels login --channel whatsapp 2>&1 || true; \
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

            let qr_output = extract_last_qr_block(&out);

            (
                StatusCode::OK,
                Json(WhatsAppQrResponse {
                    ok: true,
                    message: "WhatsApp login output captured.".into(),
                    qr_output,
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

    let key = match config::resolve_key_path(&key_path) {
      Ok(path) => path,
      Err(err) => {
        return (
          StatusCode::BAD_REQUEST,
          Json(WhatsAppRepairResponse {
            ok: false,
            message: err.to_string(),
            repair_output: String::new(),
          }),
        )
      }
    };
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

    let key = match config::resolve_key_path(&key_path) {
      Ok(path) => path,
      Err(err) => {
        return (
          StatusCode::BAD_REQUEST,
          Json(DockerFixResponse {
            ok: false,
            message: err.to_string(),
            fix_output: String::new(),
          }),
        )
      }
    };
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
  let conn = match lock_db(&state.db) {
    Ok(conn) => conn,
    Err(resp) => return resp,
  };
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
  let conn = match lock_db(&state.db) {
    Ok(conn) => conn,
    Err(resp) => return resp,
  };
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

async fn destroy_deployment_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<DestroyDeploymentRequest>,
) -> Response {
    // Look up deployment to get provider and hostname
    let (provider, hostname) = {
      let conn = match lock_db(&state.db) {
        Ok(conn) => conn,
        Err(resp) => return resp,
      };
        match db::get_deployment_by_id(&conn, &id) {
            Ok(Some(d)) => (
                d.provider.unwrap_or_default(),
                d.hostname.unwrap_or_default(),
            ),
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(DestroyDeploymentResponse {
                        ok: false,
                        message: "Deployment not found.".into(),
                    }),
                )
                .into_response()
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(DestroyDeploymentResponse {
                        ok: false,
                        message: format!("DB error: {e}"),
                    }),
                )
                    .into_response()
            }
        }
    };

    // Build destroy params and run
    let destroy_params = crate::commands::destroy::DestroyParams {
        provider: provider.clone(),
        do_token: req.do_token,
        tencent_secret_id: req.tencent_secret_id,
        tencent_secret_key: req.tencent_secret_key,
      aws_access_key_id: req.aws_access_key_id,
      aws_secret_access_key: req.aws_secret_access_key,
        aws_region: if req.aws_region.is_empty() {
            "ap-southeast-1".to_string()
        } else {
            req.aws_region
        },
        azure_tenant_id: req.azure_tenant_id,
        azure_subscription_id: req.azure_subscription_id,
        azure_client_id: req.azure_client_id,
        azure_client_secret: req.azure_client_secret,
        azure_resource_group: req.azure_resource_group,
        byteplus_access_key: req.byteplus_access_key,
        byteplus_secret_key: req.byteplus_secret_key,
        name: hostname.clone(),
        yes: true, // skip interactive confirmation
    };

    let destroy_result = crate::commands::destroy::run(destroy_params).await;

    let (cloud_ok, cloud_msg) = match &destroy_result {
        Ok(_) => (
            true,
            format!("Instance '{hostname}' destroyed on {provider}."),
        ),
        Err(e) => {
            let err_str = e.to_string().to_lowercase();
            let not_found = err_str.contains("not found")
                || err_str.contains("does not exist")
                || err_str.contains("no instance")
                || err_str.contains("404");
            if not_found {
                (
                    true,
                    format!("Instance '{hostname}' not found on {provider} (already deleted)."),
                )
            } else {
                (false, format!("Failed to destroy instance: {e}"))
            }
        }
    };

    if cloud_ok {
        // Clean up local DB record and JSON deploy file
        {
            let conn = match lock_db(&state.db) {
                Ok(conn) => conn,
                Err(resp) => return resp,
            };
            let _ = db::delete_deployment(&conn, &id);
        }
        if let Ok(deploys_dir) = config::deploys_dir() {
            let json_path = deploys_dir.join(format!("{id}.json"));
            let _ = std::fs::remove_file(json_path);
        }

        (
            StatusCode::OK,
            Json(DestroyDeploymentResponse {
                ok: true,
                message: format!("{cloud_msg} Local record deleted."),
            }),
        )
        .into_response()
    } else {
        (
            StatusCode::BAD_GATEWAY,
            Json(DestroyDeploymentResponse {
                ok: false,
                message: cloud_msg,
            }),
        )
        .into_response()
    }
}

async fn snapshot_deployment_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SnapshotDeploymentRequest>,
) -> Response {
    let (provider, hostname, region) = {
    let conn = match lock_db(&state.db) {
      Ok(conn) => conn,
      Err(resp) => return resp,
    };
        match db::get_deployment_by_id(&conn, &id) {
            Ok(Some(d)) => (
                d.provider.unwrap_or_default(),
                d.hostname.unwrap_or_default(),
                d.region.unwrap_or_default(),
            ),
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(SnapshotDeploymentResponse {
                        ok: false,
                        message: "Deployment not found.".into(),
                        operation_id: None,
                    }),
                )
                    .into_response()
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(SnapshotDeploymentResponse {
                        ok: false,
                        message: format!("DB error: {e}"),
                        operation_id: None,
                    }),
                )
                    .into_response()
            }
        }
    };

    // Validate provider-specific credentials up front
    match provider.as_str() {
        "digitalocean" => {
            if req.do_token.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(SnapshotDeploymentResponse {
                        ok: false,
                        message: "do_token is required.".into(),
                        operation_id: None,
                    }),
                )
                .into_response();
            }
        }
        "byteplus" => {
            if req.byteplus_access_key.is_empty() || req.byteplus_secret_key.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(SnapshotDeploymentResponse {
                        ok: false,
                        message: "BytePlus access key and secret key are required.".into(),
                        operation_id: None,
                    }),
                    )
                    .into_response();
            }
        }
        "lightsail" => {}
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SnapshotDeploymentResponse {
                    ok: false,
                    message: format!("Snapshot not supported for provider '{other}'."),
                    operation_id: None,
                }),
                )
                .into_response();
        }
    }

    // Spawn async task with SSE progress
    let op_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let jobs = state.jobs.clone();
    let db_handle = state.db.clone();

    // Insert operation record
    if let Ok(conn) = db_handle.lock() {
        let _ = db::insert_deployment(
            &conn, &op_id, "snapshot", "", &provider, &region, "", &hostname,
        );
    }

    let job = DeployJob {
        status: JobStatus::Running,
        rx: Some(rx),
    };
    jobs.write().await.insert(op_id.clone(), job);

    let op_id_clone = op_id.clone();
    let jobs_clone = jobs.clone();
    let db_clone = db_handle.clone();

    tokio::spawn(async move {
        let result = match provider.as_str() {
            "digitalocean" => {
                // Find droplet ID by hostname
                let client = match clawmacdo_cloud::digitalocean::DoClient::new(&req.do_token) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(format!("SNAPSHOT_ERROR:{e}"));
                        if let Ok(conn) = db_clone.lock() {
                            let _ = db::update_deployment_status(
                                &conn,
                                &op_id_clone,
                                "failed",
                                None,
                                None,
                            );
                        }
                        if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                            job.status = JobStatus::Failed;
                        }
                        return;
                    }
                };
                let droplets = match client.list_droplets().await {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = tx.send(format!("SNAPSHOT_ERROR:Failed to list droplets: {e}"));
                        if let Ok(conn) = db_clone.lock() {
                            let _ = db::update_deployment_status(
                                &conn,
                                &op_id_clone,
                                "failed",
                                None,
                                None,
                            );
                        }
                        if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                            job.status = JobStatus::Failed;
                        }
                        return;
                    }
                };
                let droplet = match droplets.iter().find(|d| d.name == hostname) {
                    Some(d) => d,
                    None => {
                        let _ = tx.send(format!("SNAPSHOT_ERROR:Droplet '{hostname}' not found."));
                        if let Ok(conn) = db_clone.lock() {
                            let _ = db::update_deployment_status(
                                &conn,
                                &op_id_clone,
                                "failed",
                                None,
                                None,
                            );
                        }
                        if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                            job.status = JobStatus::Failed;
                        }
                        return;
                    }
                };
                crate::commands::do_snapshot::run(crate::commands::do_snapshot::DoSnapshotParams {
                    do_token: req.do_token,
                    droplet_id: droplet.id,
                    snapshot_name: req.snapshot_name.clone(),
                    power_off: false,
                    progress_tx: Some(tx.clone()),
                    db: Some(db_clone.clone()),
                    op_id: Some(op_id_clone.clone()),
                })
                .await
            }
            "lightsail" => {
                let ls_region = if req.aws_region.is_empty() {
                    if region.is_empty() {
                        "ap-southeast-1".to_string()
                    } else {
                        region.clone()
                    }
                } else {
                    req.aws_region.clone()
                };
                crate::commands::ls_snapshot::run(crate::commands::ls_snapshot::LsSnapshotParams {
                    instance_name: hostname.clone(),
                    snapshot_name: req.snapshot_name.clone(),
                    region: ls_region,
                    progress_tx: Some(tx.clone()),
                    db: Some(db_clone.clone()),
                    op_id: Some(op_id_clone.clone()),
                })
                .await
            }
            "byteplus" => {
                let bp_region = if req.byteplus_region.is_empty() {
                    if region.is_empty() {
                        "ap-southeast-1".to_string()
                    } else {
                        region.clone()
                    }
                } else {
                    req.byteplus_region.clone()
                };
                let bp_client = match clawmacdo_cloud::byteplus::BytePlusClient::new(
                    &req.byteplus_access_key,
                    &req.byteplus_secret_key,
                    &bp_region,
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(format!("SNAPSHOT_ERROR:{e}"));
                        if let Ok(conn) = db_clone.lock() {
                            let _ = db::update_deployment_status(
                                &conn,
                                &op_id_clone,
                                "failed",
                                None,
                                None,
                            );
                        }
                        if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                            job.status = JobStatus::Failed;
                        }
                        return;
                    }
                };
                let instances = match bp_client.list_openclaw_instances().await {
                    Ok(i) => i,
                    Err(e) => {
                        let _ = tx.send(format!("SNAPSHOT_ERROR:Failed to list instances: {e}"));
                        if let Ok(conn) = db_clone.lock() {
                            let _ = db::update_deployment_status(
                                &conn,
                                &op_id_clone,
                                "failed",
                                None,
                                None,
                            );
                        }
                        if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                            job.status = JobStatus::Failed;
                        }
                        return;
                    }
                };
                let instance_id = match instances.iter().find(|i| i.name == hostname) {
                    Some(i) => i.id.clone(),
                    None => {
                        let _ = tx.send(format!("SNAPSHOT_ERROR:Instance '{hostname}' not found."));
                        if let Ok(conn) = db_clone.lock() {
                            let _ = db::update_deployment_status(
                                &conn,
                                &op_id_clone,
                                "failed",
                                None,
                                None,
                            );
                        }
                        if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                            job.status = JobStatus::Failed;
                        }
                        return;
                    }
                };
                crate::commands::bp_snapshot::run(crate::commands::bp_snapshot::BpSnapshotParams {
                    access_key: req.byteplus_access_key,
                    secret_key: req.byteplus_secret_key,
                    instance_id,
                    snapshot_name: req.snapshot_name.clone(),
                    region: bp_region,
                    progress_tx: Some(tx.clone()),
                    db: Some(db_clone.clone()),
                    op_id: Some(op_id_clone.clone()),
                })
                .await
            }
            _ => return,
        };

        match result {
            Ok(_) => {
                let payload = serde_json::json!({
                    "snapshot_name": req.snapshot_name,
                    "hostname": hostname,
                })
                .to_string();
                let _ = tx.send(format!("SNAPSHOT_COMPLETE_JSON:{payload}"));
                if let Ok(conn) = db_clone.lock() {
                    let _ = db::update_deployment_status(
                        &conn,
                        &op_id_clone,
                        "completed",
                        None,
                        Some(&hostname),
                    );
                }
                if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                    job.status = JobStatus::Completed;
                }
            }
            Err(e) => {
                let _ = tx.send(format!("SNAPSHOT_ERROR:{e:#}"));
                if let Ok(conn) = db_clone.lock() {
                    let _ = db::update_deployment_status(&conn, &op_id_clone, "failed", None, None);
                }
                if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                    job.status = JobStatus::Failed;
                }
            }
        }
    });

    (
        StatusCode::OK,
        Json(SnapshotDeploymentResponse {
            ok: true,
            message: "Snapshot operation started.".into(),
            operation_id: Some(op_id),
        }),
    )
    .into_response()
}

async fn toggle_funnel_handler(
    Path(id): Path<String>,
    Json(req): Json<FunnelToggleRequest>,
) -> impl IntoResponse {
    match crate::commands::tailscale_funnel::funnel_toggle(&id, &req.action, req.port).await {
        Ok((ok, message, funnel_url, gateway_token)) => (
            if ok {
                StatusCode::OK
            } else {
                StatusCode::BAD_REQUEST
            },
            Json(FunnelToggleResponse {
                ok,
                message,
                funnel_url,
                gateway_token,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(FunnelToggleResponse {
                ok: false,
                message: format!("Funnel toggle failed: {e}"),
                funnel_url: None,
                gateway_token: None,
            }),
        ),
    }
}

/// Check Tailscale Funnel status for a deployment.
async fn funnel_status_handler(Path(id): Path<String>) -> impl IntoResponse {
    let (ip, key) = match resolve_deploy_connection(&id) {
        Ok(v) => v,
        Err(_) => {
            return Json(serde_json::json!({
                "ok": false, "active": false, "funnel_url": null
            }))
        }
    };

    let cmd = "tailscale funnel status 2>&1";
    match ssh_root_async(&ip, &key, cmd).await {
        Ok(out) => {
            let trimmed = out.trim();
            // Check if funnel is active (has a proxy line)
            let has_proxy = trimmed.lines().any(|l| l.contains("proxy"));
            let funnel_url = trimmed.lines().find_map(|line| {
                let t = line.trim();
                if t.starts_with("https://") {
                    // Strip trailing ":" and annotations like " (funnel on):"
                    let url = t.trim_end_matches(':');
                    let url = if let Some(idx) = url.find(" (") {
                        &url[..idx]
                    } else {
                        url
                    };
                    Some(url.to_string())
                } else {
                    None
                }
            });
            // If funnel is active, read auth token and build one-click auth URL
            let (auth_url, gateway_token) = if has_proxy {
                if let Some(ref base_url) = funnel_url {
                    let home = config::OPENCLAW_HOME;
                    let token_cmd = format!(
                        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
                         export HOME=\"{home}\" && \
                         node -e \"const fs=require('fs');try{{const c=JSON.parse(fs.readFileSync('{home}/.openclaw/openclaw.json','utf8'));console.log((c.gateway&&c.gateway.auth&&c.gateway.auth.token)||'')}}catch(e){{console.log('')}}\""
                    );
                    let token = ssh_as_openclaw_async(&ip, &key, &token_cmd)
                        .await
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if token.is_empty() {
                        (Some(base_url.clone()), None)
                    } else {
                        (Some(format!("{base_url}/auth.html#{token}")), Some(token))
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            };

            Json(serde_json::json!({
                "ok": true,
                "active": has_proxy,
                "funnel_url": auth_url.or(funnel_url),
                "gateway_token": gateway_token,
            }))
        }
        Err(_) => Json(serde_json::json!({
            "ok": false, "active": false, "funnel_url": null
        })),
    }
}

/// Auto-approve all pending OpenClaw device pairing requests.
async fn device_approve_handler(Path(id): Path<String>) -> impl IntoResponse {
    let (ip, key) = match resolve_deploy_connection(&id) {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({"ok": false, "message": e, "approved": 0})),
    };
    let home = config::OPENCLAW_HOME;
    // Approve devices by moving entries from pending.json to paired.json directly
    // (the `openclaw devices approve` CLI fails with "pairing required" chicken-and-egg)
    let cmd = format!(
        r#"export HOME="{home}" && node -e "
const fs=require('fs');
const pf='{home}/.openclaw/devices/pending.json';
const af='{home}/.openclaw/devices/paired.json';
let pending={{}};try{{pending=JSON.parse(fs.readFileSync(pf,'utf8'))}}catch(e){{}}
let paired={{}};try{{paired=JSON.parse(fs.readFileSync(af,'utf8'))}}catch(e){{}}
const keys=Object.keys(pending);
for(const k of keys){{paired[k]=pending[k]}}
if(keys.length>0){{
  fs.writeFileSync(af,JSON.stringify(paired,null,2)+'\n');
  fs.writeFileSync(pf,'{{}}\n');
}}
console.log('APPROVED='+keys.length);
""#,
    );
    let output = ssh_as_openclaw_async(&ip, &key, &cmd)
        .await
        .unwrap_or_default();
    let count = output
        .lines()
        .find_map(|l| l.strip_prefix("APPROVED="))
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);
    Json(
        serde_json::json!({"ok": true, "message": format!("Approved {count} device(s)"), "approved": count}),
    )
}

/// Resolve a deploy record by ID, returning (ip, ssh_key_path).
fn resolve_deploy_connection(id: &str) -> Result<(String, PathBuf), String> {
    let deploys_dir = match config::deploys_dir() {
        Ok(d) => d,
        Err(e) => return Err(format!("Cannot find deploys dir: {e}")),
    };
    if !deploys_dir.exists() {
        return Err("No deploy records found.".into());
    }
    for entry in std::fs::read_dir(&deploys_dir).map_err(|e| e.to_string())? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let record: config::DeployRecord = match serde_json::from_str(&contents) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if record.id == id || record.hostname == id || record.ip_address == id {
          let key_path = match config::resolve_key_path(&record.ssh_key_path) {
            Ok(path) => path,
            Err(_) => continue,
          };
          return Ok((record.ip_address, key_path));
        }
    }
    Err(format!("No deploy record found for '{id}'."))
}

/// WhatsApp repair handler for deployments tab — resolves connection from deploy ID.
async fn deployment_whatsapp_repair_handler(Path(id): Path<String>) -> impl IntoResponse {
    let (ip, key) = match resolve_deploy_connection(&id) {
        Ok(v) => v,
        Err(msg) => {
            return (
                StatusCode::NOT_FOUND,
                Json(WhatsAppRepairResponse {
                    ok: false,
                    message: msg,
                    repair_output: String::new(),
                }),
            )
        }
    };

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

/// WhatsApp QR handler for deployments tab — resolves connection from deploy ID.
async fn deployment_whatsapp_qr_handler(Path(id): Path<String>) -> impl IntoResponse {
    let (ip, key) = match resolve_deploy_connection(&id) {
        Ok(v) => v,
        Err(msg) => {
            return (
                StatusCode::NOT_FOUND,
                Json(WhatsAppQrResponse {
                    ok: false,
                    message: msg,
                    qr_output: String::new(),
                }),
            )
        }
    };

    // Use 45s timeout — enough to capture the first QR code (appears in ~10-15s)
    let cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if command -v timeout >/dev/null 2>&1; then \
           timeout 45s openclaw channels login --channel whatsapp 2>&1 || true; \
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
                        message: "WhatsApp channel unsupported. Click 'Repair' first.".into(),
                        qr_output: out,
                    }),
                );
            }

            // Extract only the last QR code block from the output
            let qr_output = extract_last_qr_block(&out);

            (
                StatusCode::OK,
                Json(WhatsAppQrResponse {
                    ok: true,
                    message: "WhatsApp login output captured.".into(),
                    qr_output,
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

async fn list_snapshots_handler(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let provider = params
        .get("provider")
        .map(|s| s.as_str())
        .unwrap_or("digitalocean");
    let snapshots: Vec<serde_json::Value> = match provider {
        #[cfg(feature = "digitalocean")]
        "digitalocean" => {
            let token = params.get("do_token").map(|s| s.as_str()).unwrap_or("");
            if token.is_empty() {
                return Json(serde_json::json!({ "error": "do_token required" }));
            }
            match clawmacdo_cloud::digitalocean::DoClient::new(token) {
                Ok(client) => match client.list_snapshots().await {
                    Ok(snaps) => snaps
                        .into_iter()
                        .map(|s| {
                            serde_json::json!({
                                "id": s.id,
                                "name": s.name,
                                "source": "",
                                "size_gb": null,
                                "status": "available",
                                "created_at": "",
                                "regions": s.regions,
                            })
                        })
                        .collect(),
                    Err(e) => return Json(serde_json::json!({ "error": format!("{e}") })),
                },
                Err(e) => return Json(serde_json::json!({ "error": format!("{e}") })),
            }
        }
        #[cfg(feature = "lightsail")]
        "lightsail" => {
            let region = params
                .get("region")
                .map(|s| s.to_string())
                .unwrap_or_else(|| "ap-southeast-1".to_string());
            let ak = params
                .get("access_key")
                .map(|s| s.to_string())
                .unwrap_or_default();
            let sk = params
                .get("secret_key")
                .map(|s| s.to_string())
                .unwrap_or_default();
            let provider = clawmacdo_cloud::lightsail_cli::LightsailCliProvider::with_credentials(
                region, ak, sk,
            );
            match provider.list_snapshots() {
                Ok(snaps) => snaps
                    .into_iter()
                    .map(|s| {
                        serde_json::json!({
                            "id": s.name.clone().unwrap_or_default(),
                            "name": s.name.unwrap_or_default(),
                            "source": s.from_instance_name.unwrap_or_default(),
                            "size_gb": s.size_in_gb,
                            "status": s.state.unwrap_or_else(|| "available".to_string()),
                            "created_at": "",
                        })
                    })
                    .collect(),
                Err(e) => return Json(serde_json::json!({ "error": format!("{e}") })),
            }
        }
        #[cfg(feature = "byteplus")]
        "byteplus" => {
            let ak = params.get("access_key").map(|s| s.as_str()).unwrap_or("");
            let sk = params.get("secret_key").map(|s| s.as_str()).unwrap_or("");
            let bp_region = params
                .get("region")
                .map(|s| s.as_str())
                .unwrap_or("ap-southeast-1");
            if ak.is_empty() || sk.is_empty() {
                return Json(serde_json::json!({ "error": "access_key and secret_key required" }));
            }
            match clawmacdo_cloud::byteplus::BytePlusClient::new(ak, sk, bp_region) {
                Ok(client) => match client.describe_snapshots(None).await {
                    Ok(snaps) => snaps
                        .into_iter()
                        .map(|s| {
                            serde_json::json!({
                                "id": s["SnapshotId"].as_str().unwrap_or(""),
                                "name": s["SnapshotName"].as_str().unwrap_or(""),
                                "source": s["VolumeId"].as_str().unwrap_or(""),
                                "size_gb": s["VolumeSize"].as_u64(),
                                "status": s["Status"].as_str().unwrap_or("available"),
                                "created_at": s["CreationTime"].as_str().unwrap_or(""),
                            })
                        })
                        .collect(),
                    Err(e) => return Json(serde_json::json!({ "error": format!("{e}") })),
                },
                Err(e) => return Json(serde_json::json!({ "error": format!("{e}") })),
            }
        }
        _ => return Json(serde_json::json!({ "error": "Unsupported provider" })),
    };

    Json(serde_json::json!({ "snapshots": snapshots }))
}

async fn restore_snapshot_handler(
    State(state): State<AppState>,
    Json(req): Json<RestoreSnapshotRequest>,
) -> impl IntoResponse {
    // Validate provider up front
    match req.provider.as_str() {
        "byteplus" => {
            if req.byteplus_access_key.is_empty() || req.byteplus_secret_key.is_empty() {
                return Json(serde_json::json!({
                    "ok": false,
                    "message": "BytePlus access key and secret key are required."
                }));
            }
        }
        "digitalocean" => {
            if req.do_token.is_empty() {
                return Json(serde_json::json!({
                    "ok": false,
                    "message": "do_token is required."
                }));
            }
        }
        "lightsail" => {}
        other => {
            return Json(serde_json::json!({
                "ok": false,
                "message": format!("Restore not supported for provider '{other}'.")
            }));
        }
    }

    let op_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let jobs = state.jobs.clone();
    let db_handle = state.db.clone();

    // Insert operation record
    if let Ok(conn) = db_handle.lock() {
        let _ = db::insert_deployment(
            &conn,
            &op_id,
            "snapshot-restore",
            "",
            &req.provider,
            "",
            "",
            "",
        );
    }

    let job = DeployJob {
        status: JobStatus::Running,
        rx: Some(rx),
    };
    jobs.write().await.insert(op_id.clone(), job);

    let op_id_clone = op_id.clone();
    let jobs_clone = jobs.clone();
    let db_clone = db_handle.clone();

    tokio::spawn(async move {
        let result = match req.provider.as_str() {
            "byteplus" => {
                let params = crate::commands::bp_restore::BpRestoreParams {
                    access_key: req.byteplus_access_key,
                    secret_key: req.byteplus_secret_key,
                    snapshot_name: req.snapshot_name.clone(),
                    region: if req.byteplus_region.is_empty() {
                        "ap-southeast-1".to_string()
                    } else {
                        req.byteplus_region
                    },
                    size: if req.size.is_empty() {
                        None
                    } else {
                        Some(req.size)
                    },
                    spot: req.spot,
                    progress_tx: Some(tx.clone()),
                    db: Some(db_clone.clone()),
                    op_id: Some(op_id_clone.clone()),
                };
                crate::commands::bp_restore::run(params)
                    .await
                    .map(|r| (r.deploy_id, r.hostname, r.ip_address, r.ssh_key_path))
            }
            "lightsail" => {
                let params = crate::commands::ls_restore::LsRestoreParams {
                    snapshot_name: req.snapshot_name.clone(),
                    region: if req.aws_region.is_empty() {
                        "ap-southeast-1".to_string()
                    } else {
                        req.aws_region
                    },
                    size: if req.size.is_empty() {
                        None
                    } else {
                        Some(req.size)
                    },
                    progress_tx: Some(tx.clone()),
                    db: Some(db_clone.clone()),
                    op_id: Some(op_id_clone.clone()),
                };
                crate::commands::ls_restore::run(params)
                    .await
                    .map(|r| (r.deploy_id, r.hostname, r.ip_address, r.ssh_key_path))
            }
            "digitalocean" => {
                let params = crate::commands::do_restore::DoRestoreParams {
                    do_token: req.do_token,
                    snapshot_name: req.snapshot_name.clone(),
                    region: Some(if req.aws_region.is_empty() {
                        "sgp1".to_string()
                    } else {
                        req.aws_region
                    }),
                    size: if req.size.is_empty() {
                        None
                    } else {
                        Some(req.size)
                    },
                    progress_tx: Some(tx.clone()),
                    db: Some(db_clone.clone()),
                    op_id: Some(op_id_clone.clone()),
                };
                crate::commands::do_restore::run(params)
                    .await
                    .map(|r| (r.deploy_id, r.hostname, r.ip_address, r.ssh_key_path))
            }
            _ => return,
        };

        match result {
            Ok((deploy_id, hostname, ip, ssh_key_path)) => {
                let payload = serde_json::json!({
                    "deploy_id": deploy_id,
                    "hostname": hostname,
                    "ip": ip,
                    "ssh_key_path": ssh_key_path,
                })
                .to_string();
                let _ = tx.send(format!("RESTORE_COMPLETE_JSON:{payload}"));
                if let Ok(conn) = db_clone.lock() {
                    let _ = db::update_deployment_status(
                        &conn,
                        &op_id_clone,
                        "completed",
                        Some(&ip),
                        Some(&hostname),
                    );
                }
                if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                    job.status = JobStatus::Completed;
                }
            }
            Err(e) => {
                let _ = tx.send(format!("RESTORE_ERROR:{e:#}"));
                if let Ok(conn) = db_clone.lock() {
                    let _ = db::update_deployment_status(&conn, &op_id_clone, "failed", None, None);
                }
                if let Some(job) = jobs_clone.write().await.get_mut(&op_id_clone) {
                    job.status = JobStatus::Failed;
                }
            }
        }
    });

    Json(serde_json::json!({
        "ok": true,
        "message": "Restore operation started.",
        "operation_id": op_id,
    }))
}

async fn config_handler() -> impl IntoResponse {
    Json(ConfigResponse {
        dry_run: is_dry_run(),
    })
}

#[cfg(feature = "byteplus")]
async fn ark_list_endpoints_handler(Json(req): Json<ArkListEndpointsRequest>) -> impl IntoResponse {
    use clawmacdo_cloud::byteplus::BytePlusClient;

    if req.access_key.is_empty() || req.secret_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ArkListEndpointsResponse {
                ok: false,
                endpoints: vec![],
                error: Some("access_key and secret_key are required.".into()),
            }),
        );
    }

    let client = match BytePlusClient::new(&req.access_key, &req.secret_key, "ap-southeast-1") {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ArkListEndpointsResponse {
                    ok: false,
                    endpoints: vec![],
                    error: Some(format!("Client init failed: {e}")),
                }),
            )
        }
    };

    match client.list_endpoints().await {
        Ok(endpoints) => (
            StatusCode::OK,
            Json(ArkListEndpointsResponse {
                ok: true,
                endpoints,
                error: None,
            }),
        ),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ArkListEndpointsResponse {
                ok: false,
                endpoints: vec![],
                error: Some(format!("Failed to list endpoints: {e}")),
            }),
        ),
    }
}

#[cfg(not(feature = "byteplus"))]
async fn ark_list_endpoints_handler(
    Json(_req): Json<ArkListEndpointsRequest>,
) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ArkListEndpointsResponse {
            ok: false,
            endpoints: vec![],
            error: Some("BytePlus support not compiled in.".into()),
        }),
    )
}

#[cfg(feature = "byteplus")]
async fn ark_api_key_handler(Json(req): Json<ArkApiKeyRequest>) -> impl IntoResponse {
    use clawmacdo_cloud::byteplus::BytePlusClient;

    if req.access_key.is_empty() || req.secret_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ArkApiKeyResponse {
                ok: false,
                api_key: String::new(),
                expires: String::new(),
                error: Some("access_key and secret_key are required.".into()),
            }),
        );
    }

    if req.resource_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ArkApiKeyResponse {
                ok: false,
                api_key: String::new(),
                expires: String::new(),
                error: Some("resource_ids is required (at least one endpoint ID).".into()),
            }),
        );
    }

    let client = match BytePlusClient::new(&req.access_key, &req.secret_key, "ap-southeast-1") {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ArkApiKeyResponse {
                    ok: false,
                    api_key: String::new(),
                    expires: String::new(),
                    error: Some(format!("Client init failed: {e}")),
                }),
            )
        }
    };

    match client
        .get_api_key(&req.resource_type, &req.resource_ids, req.duration_seconds)
        .await
    {
        Ok((api_key, expired_time)) => {
            let expires = if expired_time > 0 {
                chrono::Utc
                    .timestamp_opt(expired_time as i64, 0)
                    .single()
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| expired_time.to_string())
            } else {
                "unknown".to_string()
            };
            (
                StatusCode::OK,
                Json(ArkApiKeyResponse {
                    ok: true,
                    api_key,
                    expires,
                    error: None,
                }),
            )
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ArkApiKeyResponse {
                ok: false,
                api_key: String::new(),
                expires: String::new(),
                error: Some(format!("Failed to generate API key: {e}")),
            }),
        ),
    }
}

#[cfg(not(feature = "byteplus"))]
async fn ark_api_key_handler(Json(_req): Json<ArkApiKeyRequest>) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ArkApiKeyResponse {
            ok: false,
            api_key: String::new(),
            expires: String::new(),
            error: Some("BytePlus support not compiled in.".into()),
        }),
    )
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

const LOGIN_HTML: &str = r##"<!DOCTYPE html>
<html lang="en" class="dark">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>ClawMacToDO — Login</title>
<script src="https://cdn.tailwindcss.com"></script>
<script>tailwind.config={darkMode:'class'}</script>
</head>
<body class="bg-gray-950 text-gray-100 min-h-screen flex items-center justify-center">
<div class="w-full max-w-sm p-8 bg-gray-900 rounded-2xl border border-gray-800 shadow-xl">
  <div class="text-center mb-6">
    <h1 class="text-2xl font-bold text-white">ClawMacToDO</h1>
    <p class="text-gray-400 text-sm mt-1">Enter your 6-digit PIN to continue</p>
  </div>
  <!-- ERROR -->
  <form method="POST" action="/login" novalidate class="space-y-4">
    <input type="password" name="pin" maxlength="6" inputmode="numeric"
      placeholder="000000" autocomplete="off"
      class="w-full px-4 py-3 bg-gray-800 border border-gray-700 rounded-lg text-center text-2xl tracking-[0.5em] font-mono text-white placeholder-gray-600 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" />
    <button type="submit"
      class="w-full py-3 bg-blue-600 hover:bg-blue-700 text-white font-semibold rounded-lg transition-colors">
      Unlock
    </button>
  </form>
</div>
</body>
</html>"##;

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
  <div class="w-full px-4 sm:px-8 lg:px-12 py-3 sm:py-4 flex items-center gap-2 sm:gap-3">
    <svg class="w-7 h-7 sm:w-8 sm:h-8 text-blue-400 shrink-0" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M5 12h14M12 5l7 7-7 7"/></svg>
    <h1 class="text-lg sm:text-xl font-bold tracking-tight">ClawMacToDO</h1>
    <span class="text-xs sm:text-sm text-slate-500 ml-1 sm:ml-2 hidden sm:inline">Deploy OpenClaw to the Cloud</span>
    <span class="ml-auto text-xs text-slate-600 font-mono hidden md:inline">v{version}</span>
    <a href="/logout" class="ml-3 text-xs text-slate-500 hover:text-red-400 transition-colors" title="Logout">&#x2716; Logout</a>
  </div>
  <!-- Tab bar -->
  <div class="w-full px-4 sm:px-8 lg:px-12 flex gap-0">
    <button id="tab-deploy" onclick="switchTab('deploy')" class="px-4 py-2 text-sm font-medium border-b-2 border-blue-500 text-blue-400 transition-colors">Deploy</button>
    <button id="tab-deployments" onclick="switchTab('deployments')" class="px-4 py-2 text-sm font-medium border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition-colors">Deployments</button>
    <button id="tab-snapshots" onclick="switchTab('snapshots')" class="px-4 py-2 text-sm font-medium border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition-colors">Snapshots</button>
  </div>
</header>

<main class="w-full px-4 sm:px-8 lg:px-12 py-6 sm:py-8">

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
    <p class="text-sm text-slate-400 mb-4">Provision OpenClaw instances across DigitalOcean, AWS Lightsail, Tencent Cloud, Microsoft Azure, and BytePlus Cloud.</p>
    <div class="flex flex-col sm:flex-row gap-2">
      <button type="button" onclick="addDeployCard()" class="w-full sm:w-auto bg-blue-600 hover:bg-blue-500 text-white font-semibold py-2 px-5 text-sm rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-blue-400 focus:ring-offset-2 focus:ring-offset-slate-900 flex items-center justify-center gap-2">
        <svg class="w-4 h-4" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15"/></svg>
        New Deployment
      </button>
      <button type="button" onclick="resetSavedDeployments()" class="w-full sm:w-auto bg-slate-700 hover:bg-slate-600 text-slate-300 font-medium py-2 px-5 text-sm rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-slate-500 focus:ring-offset-2 focus:ring-offset-slate-900">
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
            <th class="px-4 py-3">Funnel</th>
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

<!-- ═══ Snapshots view ═══ -->
<div id="snapshots-view" class="hidden">
  <div class="mb-4 flex flex-wrap gap-3 items-end">
    <div>
      <label class="block text-xs text-slate-400 mb-1">Provider</label>
      <select id="snap-provider" class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 focus:ring-blue-500 focus:border-blue-500">
        <option value="digitalocean">DigitalOcean</option>
        <option value="lightsail">AWS Lightsail</option>
        <option value="byteplus">BytePlus</option>
      </select>
    </div>
    <div id="snap-cred-do">
      <label class="block text-xs text-slate-400 mb-1">DO Token</label>
      <input type="password" id="snap-do-token" placeholder="dop_v1_..." class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 w-64" />
    </div>
    <div id="snap-cred-ls" class="hidden">
      <label class="block text-xs text-slate-400 mb-1">Access Key ID</label>
      <input type="password" id="snap-ls-ak" placeholder="AKIA..." class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 w-48" />
    </div>
    <div id="snap-cred-ls2" class="hidden">
      <label class="block text-xs text-slate-400 mb-1">Secret Access Key</label>
      <input type="password" id="snap-ls-sk" class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 w-48" />
    </div>
    <div id="snap-cred-ls3" class="hidden">
      <label class="block text-xs text-slate-400 mb-1">Region</label>
      <input type="text" id="snap-ls-region" value="ap-southeast-1" class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 w-40" />
    </div>
    <div id="snap-cred-bp" class="hidden">
      <label class="block text-xs text-slate-400 mb-1">Access Key</label>
      <input type="password" id="snap-bp-ak" placeholder="AKLT..." class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 w-40" />
    </div>
    <div id="snap-cred-bp2" class="hidden">
      <label class="block text-xs text-slate-400 mb-1">Secret Key</label>
      <input type="password" id="snap-bp-sk" class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 w-40" />
    </div>
    <div id="snap-cred-bp3" class="hidden">
      <label class="block text-xs text-slate-400 mb-1">Region</label>
      <input type="text" id="snap-bp-region" value="ap-southeast-1" class="bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 w-40" />
    </div>
    <button onclick="loadSnapshots()" class="px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white text-sm font-medium rounded-lg transition-colors">Load Snapshots</button>
  </div>
  <div class="bg-slate-900 border border-slate-800 rounded-xl shadow-xl overflow-hidden">
    <div class="overflow-x-auto">
      <table class="w-full text-sm text-left">
        <thead class="bg-slate-800 text-slate-400 text-xs uppercase tracking-wider">
          <tr>
            <th class="px-4 py-3">Name</th>
            <th class="px-4 py-3">ID</th>
            <th class="px-4 py-3">Provider</th>
            <th class="px-4 py-3">Source</th>
            <th class="px-4 py-3">Size</th>
            <th class="px-4 py-3">Status</th>
            <th class="px-4 py-3">Regions</th>
            <th class="px-4 py-3">Created</th>
            <th class="px-4 py-3">Actions</th>
          </tr>
        </thead>
        <tbody id="snapshots-tbody" class="divide-y divide-slate-800"></tbody>
      </table>
    </div>
    <div id="snapshots-empty" class="hidden px-4 py-8 text-center text-slate-500 text-sm">No snapshots found. Select a provider and click Load Snapshots.</div>
    <div id="snapshots-loading" class="hidden px-4 py-8 text-center text-slate-400 text-sm">Loading snapshots...</div>
  </div>
</div><!-- /snapshots-view -->

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
  anthropic: { keyField: 'anthropic_key',        label: 'Anthropic Key / Setup Token', placeholder: 'sk-ant-api-... or sk-ant-oat-...' },
  openai:    { keyField: 'openai_key',           label: 'OpenAI API Key',              placeholder: 'sk-...' },
  gemini:    { keyField: 'gemini_key',           label: 'Gemini API Key',              placeholder: 'AI...' },
  byteplus:  { keyField: 'byteplus_ark_api_key', label: 'BytePlus ARK API Key',        placeholder: 'AKLT...' },
};
const ALL_MODELS = ['anthropic', 'openai', 'gemini', 'byteplus'];

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
  html += '<div class="relative flex gap-2"><input type="password" name="' + pDef.keyField + '" data-model-key="primary" data-required class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 sm:px-4 py-2 sm:py-2.5 pr-10 sm:pr-12 text-sm sm:text-base text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent" placeholder="' + pDef.placeholder + '" value="' + esc(saved[pDef.keyField] || '') + '">' + eyeBtn();
  if (primary === 'byteplus') {
    html += '<button type="button" onclick="generateArkKey(' + n + ')" class="shrink-0 px-3 py-2 bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium rounded-lg transition-colors" title="Generate ARK API Key from BytePlus credentials">Generate</button>';
  }
  html += '</div></div>';
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
            <option value="byteplus">BytePlus Cloud</option>
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
        <div id="bp-creds-${n}" style="display:none" class="space-y-4">
          ${passwordField('byteplus_access_key', 'BytePlus Access Key', 'AKLT...', true)}
          ${passwordField('byteplus_secret_key', 'BytePlus Secret Key', '...', true)}
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
        <label class="flex items-center gap-3 cursor-pointer">
          <input type="checkbox" name="spot" class="w-4 h-4 rounded border-slate-600 bg-slate-800 text-amber-500 focus:ring-amber-500 focus:ring-offset-0">
          <span class="text-sm text-slate-300">Use spot instance — BytePlus only, up to ~80% cheaper (may be reclaimed)</span>
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

async function generateArkKey(n) {
  const card = document.getElementById('deploy-card-' + n);
  if (!card) return;

  const akInput = card.querySelector('[name="byteplus_access_key"]');
  const skInput = card.querySelector('[name="byteplus_secret_key"]');
  const arkInput = card.querySelector('[name="byteplus_ark_api_key"]');
  if (!akInput || !skInput || !arkInput) { alert('BytePlus credential fields not found.'); return; }

  const ak = akInput.value.trim();
  const sk = skInput.value.trim();
  if (!ak || !sk) { alert('Please enter BytePlus Access Key and Secret Key first.'); return; }

  // Step 1: List endpoints to get the first endpoint ID
  const genBtn = card.querySelector('button[onclick*="generateArkKey"]');
  const origText = genBtn ? genBtn.textContent : '';
  if (genBtn) { genBtn.textContent = 'Listing...'; genBtn.disabled = true; }

  try {
    const listResp = await fetch('/api/ark/endpoints', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ access_key: ak, secret_key: sk })
    });
    const listData = await listResp.json();

    if (!listData.ok || !listData.endpoints || listData.endpoints.length === 0) {
      alert('No ARK endpoints found. Create one in the BytePlus ARK console first.' + (listData.error ? '\\n' + listData.error : ''));
      return;
    }

    // Let user pick if multiple endpoints
    let endpointId;
    if (listData.endpoints.length === 1) {
      endpointId = listData.endpoints[0].Id || listData.endpoints[0].EndpointId;
    } else {
      let msg = 'Select an endpoint:\\n';
      listData.endpoints.forEach((ep, i) => {
        const id = ep.Id || ep.EndpointId || '-';
        const name = ep.Name || ep.EndpointName || '-';
        const model = (ep.ModelReference && (ep.ModelReference.ModelId || ep.ModelReference.FoundationModel)) || ep.Model || '-';
        msg += (i + 1) + '. ' + name + ' (' + id + ') - ' + model + '\\n';
      });
      const choice = prompt(msg + '\\nEnter number (1-' + listData.endpoints.length + '):');
      if (!choice) return;
      const idx = parseInt(choice) - 1;
      if (idx < 0 || idx >= listData.endpoints.length) { alert('Invalid selection.'); return; }
      const ep = listData.endpoints[idx];
      endpointId = ep.Id || ep.EndpointId;
    }

    if (!endpointId) { alert('Could not determine endpoint ID.'); return; }

    // Step 2: Generate API key
    if (genBtn) genBtn.textContent = 'Generating...';

    const keyResp = await fetch('/api/ark/api-key', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        access_key: ak,
        secret_key: sk,
        resource_type: 'endpoint',
        resource_ids: [endpointId],
        duration_seconds: 2592000
      })
    });
    const keyData = await keyResp.json();

    if (!keyData.ok) {
      alert('Failed to generate API key: ' + (keyData.error || 'Unknown error'));
      return;
    }

    arkInput.value = keyData.api_key;
    arkInput.type = 'text';
    alert('ARK API Key generated!\\nExpires: ' + keyData.expires);
  } catch (err) {
    alert('Error: ' + err.message);
  } finally {
    if (genBtn) { genBtn.textContent = origText; genBtn.disabled = false; }
  }
}

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
  const bpCreds = document.getElementById('bp-creds-' + n);
  const regionSel = document.getElementById('region-' + n);
  const sizeSel = document.getElementById('size-' + n);

  // Hide all credential sections and clear required
  doCreds.style.display = 'none';
  tcCreds.style.display = 'none';
  awsCreds.style.display = 'none';
  azureCreds.style.display = 'none';
  bpCreds.style.display = 'none';
  doCreds.querySelectorAll('input').forEach(i => { i.required = false; i.value = ''; });
  tcCreds.querySelectorAll('input').forEach(i => { i.required = false; i.value = ''; });
  awsCreds.querySelectorAll('input').forEach(i => { i.required = false; i.value = ''; });
  azureCreds.querySelectorAll('input').forEach(i => { i.required = false; i.value = ''; });
  bpCreds.querySelectorAll('input').forEach(i => { i.required = false; i.value = ''; });

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
  } else if (provider === 'byteplus') {
    bpCreds.style.display = 'block';
    bpCreds.querySelectorAll('input').forEach(i => i.required = true);
    regionSel.innerHTML = `
      <option value="ap-southeast-1" selected>ap-southeast-1 (Singapore)</option>
    `;
    sizeSel.innerHTML = `
      <option value="ecs.c3i.large">ecs.c3i.large (2 vCPU, 4 GB)</option>
      <option value="ecs.g3i.large" selected>ecs.g3i.large (2 vCPU, 8 GB)</option>
      <option value="ecs.c3i.xlarge">ecs.c3i.xlarge (4 vCPU, 8 GB)</option>
      <option value="ecs.g3i.xlarge">ecs.g3i.xlarge (4 vCPU, 16 GB)</option>
    `;
    // Auto-select BytePlus ARK as default AI model
    const modelContainer = document.getElementById('model-selectors-' + n);
    if (modelContainer) {
      const pSel = modelContainer.querySelector('[data-model-slot=primary]');
      if (pSel) { pSel.value = 'byteplus'; syncModelSelectors(n); }
    }
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
      <div class="bg-amber-500/10 border border-amber-500/30 rounded-lg p-3 mb-3">
        <p class="text-xs text-amber-300 font-medium mb-1">Common Error</p>
        <p class="text-[10px] text-amber-200/80 font-mono leading-relaxed">Agent failed before reply: Failed to inspect sandbox image: permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock</p>
        <p class="text-xs text-amber-300 mt-2">Click the button below to fix this error.</p>
      </div>
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
    output.textContent = details ? (msg + '\n\n' + details) : msg;
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
    // Auto-fetch QR after successful repair
    if (res.ok && data.ok) {
      btn.disabled = false;
      btn.textContent = oldText;
      const qrBtn = panel.querySelector('.whatsapp-qr-btn');
      if (qrBtn) {
        output.textContent += '\n\nAuto-fetching WhatsApp QR code...';
        fetchWhatsAppQr(qrBtn);
      }
      return;
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
    byteplus_access_key: val('byteplus_access_key'),
    byteplus_secret_key: val('byteplus_secret_key'),
    byteplus_ark_api_key: val('byteplus_ark_api_key'),
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
    spot: form.querySelector('[name="spot"]').checked,
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
const TAB_INACTIVE = 'px-4 py-2 text-sm font-medium border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition-colors';
const TAB_ACTIVE = 'px-4 py-2 text-sm font-medium border-b-2 border-blue-500 text-blue-400 transition-colors';
function switchTab(tab) {
  const views = ['deploy', 'deployments', 'snapshots'];
  views.forEach(v => {
    const el = document.getElementById(v + '-view');
    const btn = document.getElementById('tab-' + v);
    if (el) el.classList.toggle('hidden', v !== tab);
    if (btn) btn.className = v === tab ? TAB_ACTIVE : TAB_INACTIVE;
    // Reset forms in hidden views
    if (v !== tab && el) el.querySelectorAll('form').forEach(f => f.reset());
  });
  // Close any open modals
  ['snapshot-modal', 'restore-modal', 'destroy-modal'].forEach(id => {
    const m = document.getElementById(id);
    if (m) m.remove();
  });
  // Reset snapshot results
  const snapTbody = document.getElementById('snapshots-tbody');
  if (tab !== 'snapshots' && snapTbody) snapTbody.innerHTML = '';
  const snapEmpty = document.getElementById('snapshots-empty');
  if (tab !== 'snapshots' && snapEmpty) snapEmpty.classList.add('hidden');
  if (tab === 'deployments') loadDeployments();
  if (tab === 'snapshots') initSnapshotCredFields();
}

// ── Deployments table ───────────────────────────────────────────────────
let depCurrentPage = 1;

function statusBadge(status, deployId) {
  const colors = {
    'completed': 'bg-green-500/20 text-green-300 border-green-500/30',
    'running':   'bg-blue-500/20 text-blue-300 border-blue-500/30',
    'failed':    'bg-red-500/20 text-red-300 border-red-500/30',
    'dry-run':   'bg-yellow-500/20 text-yellow-300 border-yellow-500/30',
  };
  const cls = colors[status] || 'bg-slate-500/20 text-slate-300 border-slate-500/30';
  let html = '<span class="inline-block px-2 py-0.5 text-xs font-medium rounded-full border ' + cls + '">' + (status || 'unknown') + '</span>';
  if (status === 'running' && deployId) {
    html += '<div id="progress-' + esc(deployId) + '" class="mt-1.5">' +
      '<div class="w-full bg-slate-700 rounded-full h-1.5 overflow-hidden">' +
        '<div class="bg-blue-500 h-1.5 rounded-full transition-all duration-500 animate-pulse" style="width:0%"></div>' +
      '</div>' +
      '<div class="text-[10px] text-slate-500 mt-0.5">loading...</div>' +
    '</div>';
  }
  return html;
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
          '<td class="px-4 py-3">' + statusBadge(d.status, d.id) + '</td>' +
          '<td class="px-4 py-3 text-slate-500 text-xs whitespace-nowrap">' + formatSGT(d.created_at) + '</td>' +
          '<td class="px-4 py-3">' +
            '<div class="flex flex-col gap-1">' +
              '<div class="flex gap-1 items-center">' +
                '<button id="funnel-btn-' + esc(d.id) + '" onclick="toggleFunnel(\'' + esc(d.id) + '\',this)" class="text-slate-400 hover:text-blue-300 text-xs font-medium px-2 py-1 rounded bg-slate-700/50 hover:bg-blue-500/20 border border-slate-600 hover:border-blue-500/30 transition-colors" title="Toggle Tailscale Funnel">Off</button>' +
                '<button id="funnel-open-' + esc(d.id) + '" onclick="openFunnel(\'' + esc(d.id) + '\')" class="hidden text-xs text-blue-400 hover:text-blue-300 px-1.5 py-0.5 rounded bg-blue-500/10 hover:bg-blue-500/20 border border-blue-500/20 ml-1" title="Open webchat (auto-approves device)">Open</button>' +
                '<input type="hidden" id="funnel-url-val-' + esc(d.id) + '" />' +
              '</div>' +
              '<div id="funnel-progress-' + esc(d.id) + '" class="hidden">' +
                '<div class="w-full bg-slate-700 rounded-full h-1 overflow-hidden">' +
                  '<div class="bg-blue-500 h-1 rounded-full transition-all duration-500 animate-pulse" style="width:0%"></div>' +
                '</div>' +
                '<div class="text-[9px] text-slate-500 mt-0.5">Verifying funnel...</div>' +
              '</div>' +
            '</div>' +
          '</td>' +
          '<td class="px-4 py-3">' +
            '<div class="flex flex-col gap-1">' +
              '<div class="grid grid-cols-2 gap-1">' +
                '<button onclick="depRepairWhatsApp(\'' + esc(d.id) + '\',this)" class="text-amber-400 hover:text-amber-300 text-xs font-medium px-2 py-1 rounded bg-amber-500/10 hover:bg-amber-500/20 border border-amber-500/20 transition-colors" title="Install/Repair WhatsApp Support">WhatsApp</button>' +
                '<button onclick="depFetchWhatsAppQr(\'' + esc(d.id) + '\',this)" class="text-blue-400 hover:text-blue-300 text-xs font-medium px-2 py-1 rounded bg-blue-500/10 hover:bg-blue-500/20 border border-blue-500/20 transition-colors" title="Fetch WhatsApp QR Code">QR</button>' +
                '<button onclick="depSnapshot(\'' + esc(d.id) + '\',\'' + esc(d.provider || '') + '\',\'' + esc(d.hostname || '') + '\')" class="text-cyan-400 hover:text-cyan-300 text-xs font-medium px-2 py-1 rounded bg-cyan-500/10 hover:bg-cyan-500/20 border border-cyan-500/20 transition-colors" title="Create snapshot">Snapshot</button>' +
                '<button onclick="refreshIp(\'' + esc(d.id) + '\',this)" class="text-purple-400 hover:text-purple-300 text-xs font-medium px-2 py-1 rounded bg-purple-500/10 hover:bg-purple-500/20 border border-purple-500/20 transition-colors" title="Refresh IP from cloud provider">Refresh IP</button>' +
                '<button onclick="showDestroyModal(\'' + esc(d.id) + '\',\'' + esc(d.provider || '') + '\',\'' + esc(d.hostname || '') + '\',\'' + esc(d.ip_address || '') + '\')" class="text-red-400 hover:text-red-300 text-xs font-medium px-2 py-1 rounded bg-red-500/10 hover:bg-red-500/20 border border-red-500/20 transition-colors">Destroy</button>' +
              '</div>' +
              '<pre id="wa-output-' + esc(d.id) + '" class="hidden mt-1 bg-slate-900 border border-slate-700 rounded p-2 font-mono text-[6px] leading-[7px] whitespace-pre text-slate-300 max-h-[80vh] overflow-auto"></pre>' +
            '</div>' +
          '</td>';
        tbody.appendChild(tr);
      }
    }
    // Pagination
    const totalPages = data.total_pages || 1;
    document.getElementById('dep-page-info').textContent = 'Page ' + depCurrentPage + ' of ' + totalPages;
    document.getElementById('dep-prev').disabled = depCurrentPage <= 1;
    document.getElementById('dep-next').disabled = depCurrentPage >= totalPages;

    // Probe funnel status for each completed deployment
    // and poll progress for running deployments
    if (data.deployments) {
      for (const d of data.deployments) {
        if (d.status === 'completed') checkFunnelStatus(d.id);
        if (d.status === 'running') pollDeployProgress(d.id);
      }
    }
  } catch (e) {
    console.error('Failed to load deployments:', e);
  }
}

// Track active progress pollers to avoid duplicates
const _activePollers = new Set();

async function pollDeployProgress(deployId) {
  if (_activePollers.has(deployId)) return;
  _activePollers.add(deployId);

  async function poll() {
    try {
      const res = await fetch('/api/deploy/steps/' + deployId);
      const data = await res.json();
      const el = document.getElementById('progress-' + deployId);
      if (!el) { _activePollers.delete(deployId); return; }

      const steps = data.steps || [];
      if (steps.length === 0) {
        el.querySelector('div > div').style.width = '5%';
        el.querySelector('div + div').textContent = 'Starting...';
      } else {
        const total = steps[0].total_steps || 16;
        const completed = steps.filter(s => s.status === 'completed').length;
        const running = steps.find(s => s.status === 'running');
        const failed = steps.find(s => s.status === 'failed');
        const pct = Math.round(((completed + (running ? 0.5 : 0)) / total) * 100);
        const bar = el.querySelector('div > div');
        const label = el.querySelector('div + div');
        bar.style.width = pct + '%';
        if (failed) {
          bar.className = 'bg-red-500 h-1.5 rounded-full transition-all duration-500';
          label.textContent = 'Step ' + failed.step_number + '/' + total + ': ' + failed.label + ' (failed)';
          label.className = 'text-[10px] text-red-400 mt-0.5';
          _activePollers.delete(deployId);
          setTimeout(() => loadDeployments(), 2000);
          return;
        } else if (running) {
          label.textContent = 'Step ' + running.step_number + '/' + total + ': ' + running.label;
        } else if (completed === total) {
          bar.style.width = '100%';
          bar.className = 'bg-green-500 h-1.5 rounded-full transition-all duration-500';
          label.textContent = 'Complete';
          label.className = 'text-[10px] text-green-400 mt-0.5';
          _activePollers.delete(deployId);
          setTimeout(() => loadDeployments(), 2000);
          return;
        } else {
          const last = steps[steps.length - 1];
          label.textContent = 'Step ' + last.step_number + '/' + total + ': ' + last.label;
        }
      }
      // Continue polling every 3 seconds
      setTimeout(poll, 3000);
    } catch (e) {
      _activePollers.delete(deployId);
    }
  }
  poll();
}

async function checkFunnelStatus(id) {
  try {
    const res = await fetch('/api/deployments/' + encodeURIComponent(id) + '/funnel/status');
    const data = await res.json();
    const btn = document.getElementById('funnel-btn-' + id);
    const openBtn = document.getElementById('funnel-open-' + id);
    const urlVal = document.getElementById('funnel-url-val-' + id);
    if (!btn) return;
    if (data.ok && data.active) {
      btn.textContent = 'On';
      btn.className = 'text-green-400 hover:text-green-300 text-xs font-medium px-2 py-1 rounded bg-green-500/20 hover:bg-green-500/30 border border-green-500/30 transition-colors';
      if (data.funnel_url && openBtn && urlVal) {
        urlVal.value = data.funnel_url;
        openBtn.classList.remove('hidden');
      }
    }
  } catch (e) {
    // Silently ignore — button stays as Off
  }
}

async function openFunnel(id) {
  const urlVal = document.getElementById('funnel-url-val-' + id);
  if (!urlVal || !urlVal.value) return;
  // dangerouslyDisableDeviceAuth is set during funnel setup — no pairing needed
  window.open(urlVal.value, '_blank');
}

function deploymentsPage(delta) {
  depCurrentPage += delta;
  if (depCurrentPage < 1) depCurrentPage = 1;
  loadDeployments();
}

function showDestroyModal(id, provider, hostname, ip) {
  // Remove existing modal if any
  const existing = document.getElementById('destroy-modal');
  if (existing) existing.remove();

  let credsHtml = '';
  if (provider === 'digitalocean') {
    credsHtml = '<div><label class="block text-sm font-medium text-slate-300 mb-1">DigitalOcean Token <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-do-token" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent" placeholder="dop_v1_...">' + eyeBtn() + '</div></div>';
  } else if (provider === 'tencent') {
    credsHtml = '<div><label class="block text-sm font-medium text-slate-300 mb-1">Tencent SecretId <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-tencent-id" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent" placeholder="AKID...">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">Tencent SecretKey <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-tencent-key" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent">' + eyeBtn() + '</div></div>';
  } else if (provider === 'lightsail') {
    credsHtml = '<div><label class="block text-sm font-medium text-slate-300 mb-1">AWS Access Key ID <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-aws-ak" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent" placeholder="AKIA...">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">AWS Secret Access Key <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-aws-sk" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">AWS Region</label><input type="text" id="destroy-aws-region" value="ap-southeast-1" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 focus:ring-2 focus:ring-red-500 focus:border-transparent"></div>';
  } else if (provider === 'azure') {
    credsHtml = '<div><label class="block text-sm font-medium text-slate-300 mb-1">Tenant ID <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-azure-tenant" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">Subscription ID <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-azure-sub" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">Client ID <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-azure-client" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">Client Secret <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-azure-secret" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">Resource Group <span class="text-red-400">*</span></label><input type="text" id="destroy-azure-rg" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 focus:ring-2 focus:ring-red-500 focus:border-transparent"></div>';
  } else if (provider === 'byteplus') {
    credsHtml = '<div><label class="block text-sm font-medium text-slate-300 mb-1">BytePlus Access Key <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-bp-ak" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent" placeholder="AKLT...">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">BytePlus Secret Key <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="destroy-bp-sk" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-red-500 focus:border-transparent">' + eyeBtn() + '</div></div>';
  } else {
    credsHtml = '<p class="text-slate-400 text-sm">Unknown provider "' + esc(provider) + '". Cannot destroy cloud instance.</p>';
  }

  const modal = document.createElement('div');
  modal.id = 'destroy-modal';
  modal.className = 'fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm';
  modal.innerHTML = `
    <div class="bg-slate-900 border border-slate-700 rounded-xl shadow-2xl max-w-md w-full mx-4 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="w-10 h-10 rounded-full bg-red-500/20 flex items-center justify-center">
          <svg class="w-5 h-5 text-red-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z"/></svg>
        </div>
        <h3 class="text-lg font-semibold text-red-400">Destroy Instance</h3>
      </div>
      <p class="text-sm text-slate-300 mb-1">This will <strong class="text-red-400">permanently destroy</strong> the cloud instance and delete the deployment record.</p>
      <div class="bg-slate-800/50 rounded-lg p-3 mb-4 text-xs text-slate-400 space-y-1">
        <div><span class="text-slate-500">Hostname:</span> <span class="text-slate-200">${esc(hostname || '-')}</span></div>
        <div><span class="text-slate-500">IP:</span> <span class="text-slate-200">${esc(ip || '-')}</span></div>
        <div><span class="text-slate-500">Provider:</span> <span class="text-slate-200">${esc(provider || '-')}</span></div>
      </div>
      <p class="text-sm text-slate-400 mb-3">Enter your cloud provider credentials to confirm:</p>
      <div id="destroy-creds" class="mb-4">${credsHtml}</div>
      <div id="destroy-status" class="text-sm mb-3 hidden"></div>
      <div class="flex gap-3 justify-end">
        <button onclick="closeDestroyModal()" class="px-4 py-2 text-sm font-medium text-slate-300 bg-slate-800 hover:bg-slate-700 border border-slate-600 rounded-lg transition-colors">Cancel</button>
        <button id="destroy-confirm-btn" onclick="confirmDestroy('${esc(id)}','${esc(provider)}')" class="px-4 py-2 text-sm font-medium text-white bg-red-600 hover:bg-red-500 rounded-lg transition-colors">Destroy Instance</button>
      </div>
    </div>
  `;
  document.body.appendChild(modal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeDestroyModal(); });
}

function closeDestroyModal() {
  const modal = document.getElementById('destroy-modal');
  if (modal) modal.remove();
}

async function confirmDestroy(id, provider) {
  const btn = document.getElementById('destroy-confirm-btn');
  const status = document.getElementById('destroy-status');
  btn.disabled = true;
  btn.textContent = 'Destroying...';
  status.className = 'text-sm mb-3 text-yellow-400';
  status.textContent = 'Destroying cloud instance... this may take a moment.';
  status.classList.remove('hidden');

  const body = {};
  if (provider === 'digitalocean') {
    body.do_token = (document.getElementById('destroy-do-token') || {}).value || '';
  } else if (provider === 'tencent') {
    body.tencent_secret_id = (document.getElementById('destroy-tencent-id') || {}).value || '';
    body.tencent_secret_key = (document.getElementById('destroy-tencent-key') || {}).value || '';
  } else if (provider === 'lightsail') {
    body.aws_access_key_id = (document.getElementById('destroy-aws-ak') || {}).value || '';
    body.aws_secret_access_key = (document.getElementById('destroy-aws-sk') || {}).value || '';
    body.aws_region = (document.getElementById('destroy-aws-region') || {}).value || 'ap-southeast-1';
  } else if (provider === 'azure') {
    body.azure_tenant_id = (document.getElementById('destroy-azure-tenant') || {}).value || '';
    body.azure_subscription_id = (document.getElementById('destroy-azure-sub') || {}).value || '';
    body.azure_client_id = (document.getElementById('destroy-azure-client') || {}).value || '';
    body.azure_client_secret = (document.getElementById('destroy-azure-secret') || {}).value || '';
    body.azure_resource_group = (document.getElementById('destroy-azure-rg') || {}).value || '';
  } else if (provider === 'byteplus') {
    body.byteplus_access_key = (document.getElementById('destroy-bp-ak') || {}).value || '';
    body.byteplus_secret_key = (document.getElementById('destroy-bp-sk') || {}).value || '';
  }

  try {
    const res = await fetch('/api/deployments/' + encodeURIComponent(id) + '/destroy', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const data = await res.json();
    if (data.ok) {
      status.className = 'text-sm mb-3 text-green-400';
      status.textContent = data.message;
      setTimeout(() => { closeDestroyModal(); loadDeployments(); }, 1500);
    } else {
      status.className = 'text-sm mb-3 text-red-400';
      status.textContent = data.message || 'Destroy failed.';
      btn.disabled = false;
      btn.textContent = 'Destroy Instance';
    }
  } catch (e) {
    status.className = 'text-sm mb-3 text-red-400';
    status.textContent = 'Network error: ' + e.message;
    btn.disabled = false;
    btn.textContent = 'Destroy Instance';
  }
}

async function toggleFunnel(id, btn) {
  const isOn = btn.textContent.trim() === 'On';
  const action = isOn ? 'off' : 'on';
  const origText = btn.textContent;
  btn.disabled = true;
  btn.textContent = action === 'on' ? 'Enabling...' : 'Disabling...';
  btn.className = 'text-yellow-400 text-xs font-medium px-2 py-1 rounded bg-yellow-500/20 border border-yellow-500/30 cursor-wait';

  try {
    const res = await fetch('/api/deployments/' + encodeURIComponent(id) + '/funnel', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ action: action, port: 18789 }),
    });
    const data = await res.json();
    const openBtn = document.getElementById('funnel-open-' + id);
    const urlVal = document.getElementById('funnel-url-val-' + id);
    if (data.ok && action === 'on') {
      btn.textContent = 'Verifying...';
      btn.className = 'text-yellow-400 text-xs font-medium px-2 py-1 rounded bg-yellow-500/20 border border-yellow-500/30 cursor-wait';
      if (data.funnel_url && urlVal) {
        urlVal.value = data.funnel_url;
      }
      // Show progress bar
      const progressEl = document.getElementById('funnel-progress-' + id);
      if (progressEl) {
        progressEl.classList.remove('hidden');
        const bar = progressEl.querySelector('div > div');
        const label = progressEl.querySelector('div + div');
        bar.style.width = '10%';
        label.textContent = 'Verifying funnel is reachable...';
      }
      // Poll funnel status until it is confirmed active before showing Open
      const funnelUrl = data.funnel_url || '';
      const maxAttempts = 10;
      let verified = false;
      for (let attempt = 0; attempt < maxAttempts; attempt++) {
        await new Promise(r => setTimeout(r, 3000));
        const pct = Math.round(((attempt + 1) / maxAttempts) * 100);
        if (progressEl) {
          const bar = progressEl.querySelector('div > div');
          const label = progressEl.querySelector('div + div');
          bar.style.width = pct + '%';
          label.textContent = 'Checking funnel status... (' + (attempt + 1) + '/' + maxAttempts + ')';
        }
        try {
          const statusRes = await fetch('/api/deployments/' + encodeURIComponent(id) + '/funnel/status');
          const statusData = await statusRes.json();
          if (statusData.ok && statusData.active) {
            verified = true;
            if (statusData.funnel_url && urlVal) urlVal.value = statusData.funnel_url;
            break;
          }
        } catch (_) {}
      }
      if (progressEl) {
        const bar = progressEl.querySelector('div > div');
        const label = progressEl.querySelector('div + div');
        bar.style.width = '100%';
        if (verified) {
          bar.className = 'bg-green-500 h-1 rounded-full transition-all duration-500';
          label.textContent = 'Funnel verified and active';
          label.className = 'text-[9px] text-green-400 mt-0.5';
        } else {
          bar.className = 'bg-amber-500 h-1 rounded-full transition-all duration-500';
          label.textContent = 'Could not verify — funnel may still be starting';
          label.className = 'text-[9px] text-amber-400 mt-0.5';
        }
        setTimeout(() => { progressEl.classList.add('hidden'); }, 5000);
      }
      if (verified) {
        btn.textContent = 'On';
        btn.className = 'text-green-400 hover:text-green-300 text-xs font-medium px-2 py-1 rounded bg-green-500/20 hover:bg-green-500/30 border border-green-500/30 transition-colors';
        if (openBtn && urlVal && urlVal.value) {
          openBtn.classList.remove('hidden');
        }
      } else {
        btn.textContent = 'On (unverified)';
        btn.className = 'text-amber-400 text-xs font-medium px-2 py-1 rounded bg-amber-500/20 border border-amber-500/30';
        if (funnelUrl && openBtn && urlVal) {
          openBtn.classList.remove('hidden');
        }
      }
    } else if (data.ok && action === 'off') {
      btn.textContent = 'Off';
      btn.className = 'text-slate-400 hover:text-blue-300 text-xs font-medium px-2 py-1 rounded bg-slate-700/50 hover:bg-blue-500/20 border border-slate-600 hover:border-blue-500/30 transition-colors';
      if (openBtn) { openBtn.classList.add('hidden'); }
      if (urlVal) { urlVal.value = ''; }
    } else {
      btn.textContent = origText;
      btn.className = 'text-red-400 text-xs font-medium px-2 py-1 rounded bg-red-500/20 border border-red-500/30';
      alert('Funnel toggle failed: ' + (data.message || 'Unknown error'));
      setTimeout(() => {
        btn.className = 'text-slate-400 hover:text-blue-300 text-xs font-medium px-2 py-1 rounded bg-slate-700/50 hover:bg-blue-500/20 border border-slate-600 hover:border-blue-500/30 transition-colors';
      }, 2000);
    }
  } catch (e) {
    btn.textContent = origText;
    btn.className = 'text-red-400 text-xs font-medium px-2 py-1 rounded bg-red-500/20 border border-red-500/30';
    alert('Network error: ' + e.message);
    setTimeout(() => {
      btn.className = 'text-slate-400 hover:text-blue-300 text-xs font-medium px-2 py-1 rounded bg-slate-700/50 hover:bg-blue-500/20 border border-slate-600 hover:border-blue-500/30 transition-colors';
    }, 2000);
  }
  btn.disabled = false;
}

async function refreshIp(id, btn) {
  const origText = btn.textContent;
  btn.disabled = true;
  btn.textContent = 'Refreshing...';
  btn.className = 'text-yellow-400 text-xs font-medium px-2 py-1 rounded bg-yellow-500/20 border border-yellow-500/30 cursor-wait';
  try {
    const res = await fetch('/api/deployments/' + encodeURIComponent(id) + '/refresh-ip', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
    });
    const data = await res.json();
    if (data.ok) {
      btn.textContent = data.old_ip ? 'Updated' : 'Unchanged';
      btn.className = 'text-green-400 text-xs font-medium px-2 py-1 rounded bg-green-500/20 border border-green-500/30';
      setTimeout(() => loadDeployments(), 1500);
    } else {
      alert('Refresh IP failed: ' + (data.message || 'Unknown error'));
      btn.textContent = origText;
      btn.className = 'text-purple-400 hover:text-purple-300 text-xs font-medium px-2 py-1 rounded bg-purple-500/10 hover:bg-purple-500/20 border border-purple-500/20 transition-colors';
    }
  } catch (e) {
    alert('Network error: ' + e.message);
    btn.textContent = origText;
    btn.className = 'text-purple-400 hover:text-purple-300 text-xs font-medium px-2 py-1 rounded bg-purple-500/10 hover:bg-purple-500/20 border border-purple-500/20 transition-colors';
  }
  btn.disabled = false;
}

async function depRepairWhatsApp(id, btn) {
  const output = document.getElementById('wa-output-' + id);
  if (!output) return;
  output.classList.remove('hidden');
  const oldText = btn.textContent;
  btn.disabled = true;
  btn.textContent = 'Repairing...';
  output.textContent = 'Updating OpenClaw and refreshing extensions...';
  try {
    const res = await fetch('/api/deployments/' + encodeURIComponent(id) + '/whatsapp/repair', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '{}',
    });
    const data = await res.json();
    const msg = data.message || 'WhatsApp repair completed.';
    const details = (data.repair_output || '').trim();
    output.textContent = details ? (msg + '\n\n' + details) : msg;
    if (res.ok && data.ok) {
      btn.disabled = false;
      btn.textContent = oldText;
      output.textContent += '\n\nAuto-fetching WhatsApp QR code (may take up to 4 minutes)...';
      depFetchWhatsAppQr(id, null);
      return;
    }
  } catch (err) {
    output.textContent = 'Request failed: ' + err.message;
  } finally {
    btn.disabled = false;
    btn.textContent = oldText;
  }
}

async function depFetchWhatsAppQr(id, btn) {
  const output = document.getElementById('wa-output-' + id);
  if (!output) return;
  output.classList.remove('hidden');
  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Fetching...';
  }
  if (btn) {
    output.textContent = 'Fetching WhatsApp QR code (may take up to 4 minutes)...';
  }
  try {
    const res = await fetch('/api/deployments/' + encodeURIComponent(id) + '/whatsapp/qr', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '{}',
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
    if (btn) {
      btn.disabled = false;
      btn.textContent = 'QR';
    }
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

// ── Snapshot tab ────────────────────────────────────────────────────────
function initSnapshotCredFields() {
  const prov = document.getElementById('snap-provider').value;
  document.getElementById('snap-cred-do').classList.toggle('hidden', prov !== 'digitalocean');
  document.getElementById('snap-cred-ls').classList.toggle('hidden', prov !== 'lightsail');
  document.getElementById('snap-cred-ls2').classList.toggle('hidden', prov !== 'lightsail');
  document.getElementById('snap-cred-ls3').classList.toggle('hidden', prov !== 'lightsail');
  document.getElementById('snap-cred-bp').classList.toggle('hidden', prov !== 'byteplus');
  document.getElementById('snap-cred-bp2').classList.toggle('hidden', prov !== 'byteplus');
  document.getElementById('snap-cred-bp3').classList.toggle('hidden', prov !== 'byteplus');
}
document.getElementById('snap-provider').addEventListener('change', initSnapshotCredFields);

async function loadSnapshots() {
  const provider = document.getElementById('snap-provider').value;
  const tbody = document.getElementById('snapshots-tbody');
  const empty = document.getElementById('snapshots-empty');
  const loading = document.getElementById('snapshots-loading');
  tbody.innerHTML = '';
  empty.classList.add('hidden');
  loading.classList.remove('hidden');

  const params = new URLSearchParams({ provider });
  if (provider === 'digitalocean') params.set('do_token', document.getElementById('snap-do-token').value);
  if (provider === 'lightsail') {
    params.set('access_key', document.getElementById('snap-ls-ak').value);
    params.set('secret_key', document.getElementById('snap-ls-sk').value);
    params.set('region', document.getElementById('snap-ls-region').value);
  }
  if (provider === 'byteplus') {
    params.set('access_key', document.getElementById('snap-bp-ak').value);
    params.set('secret_key', document.getElementById('snap-bp-sk').value);
    params.set('region', document.getElementById('snap-bp-region').value);
  }

  try {
    const res = await fetch('/api/snapshots?' + params.toString(), {
      headers: { 'Content-Type': 'application/json' },
    });
    const data = await res.json();
    loading.classList.add('hidden');
    if (!res.ok) { alert(data.error || 'Failed to load snapshots'); return; }
    if (!data.snapshots || data.snapshots.length === 0) {
      empty.classList.remove('hidden');
      return;
    }
    for (const s of data.snapshots) {
      const tr = document.createElement('tr');
      tr.className = 'hover:bg-slate-800/50';
      tr.innerHTML =
        '<td class="px-4 py-3 text-slate-200">' + esc(s.name || '-') + '</td>' +
        '<td class="px-4 py-3 text-slate-400 font-mono text-xs">' + esc(s.id || '-') + '</td>' +
        '<td class="px-4 py-3 text-slate-400">' + esc(provider) + '</td>' +
        '<td class="px-4 py-3 text-slate-400 text-xs">' + esc(s.source || '-') + '</td>' +
        '<td class="px-4 py-3 text-slate-400 text-xs">' + (s.size_gb ? s.size_gb + ' GB' : '-') + '</td>' +
        '<td class="px-4 py-3">' + statusBadge(s.status || 'available') + '</td>' +
        '<td class="px-4 py-3 text-slate-400 text-xs">' + (s.regions ? esc(s.regions.join(', ')) : '-') + '</td>' +
        '<td class="px-4 py-3 text-slate-500 text-xs whitespace-nowrap">' + esc(s.created_at || '-') + '</td>' +
        '<td class="px-4 py-3">' +
          '<button onclick="restoreSnapshot(\'' + esc(s.name || '') + '\',\'' + esc(provider) + '\')" class="text-green-400 hover:text-green-300 text-xs font-medium px-2 py-1 rounded bg-green-500/10 hover:bg-green-500/20 border border-green-500/20 transition-colors">Restore</button>' +
        '</td>';
      tbody.appendChild(tr);
    }
  } catch (e) {
    loading.classList.add('hidden');
    alert('Error loading snapshots: ' + e.message);
  }
}

function restoreSnapshot(name, provider) {
  const existing = document.getElementById('restore-modal');
  if (existing) existing.remove();

  let credsHtml = '';
  if (provider === 'digitalocean') {
    credsHtml = '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">DigitalOcean Token <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="restore-do-token" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-green-500 focus:border-transparent" placeholder="dop_v1_...">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">Region</label><input type="text" id="restore-do-region" value="sgp1" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 focus:ring-2 focus:ring-green-500 focus:border-transparent"></div>';
  } else if (provider === 'lightsail') {
    credsHtml = '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">AWS Region</label><input type="text" id="restore-aws-region" value="ap-southeast-1" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 focus:ring-2 focus:ring-green-500 focus:border-transparent"></div>';
  } else if (provider === 'byteplus') {
    credsHtml = '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">BytePlus Access Key <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="restore-bp-ak" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-green-500 focus:border-transparent" placeholder="AKLT...">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">BytePlus Secret Key <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="restore-bp-sk" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-green-500 focus:border-transparent">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">Region</label><input type="text" id="restore-bp-region" value="ap-southeast-1" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 focus:ring-2 focus:ring-green-500 focus:border-transparent"></div>' +
      '<div class="mt-3"><label class="flex items-center gap-2 text-sm text-slate-300 cursor-pointer"><input type="checkbox" id="restore-bp-spot" class="rounded border-slate-600 bg-slate-800 text-green-500 focus:ring-green-500"> Use spot instance (cheaper)</label></div>';
  }

  const modal = document.createElement('div');
  modal.id = 'restore-modal';
  modal.className = 'fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm';
  modal.innerHTML = `
    <div class="bg-slate-900 border border-slate-700 rounded-xl shadow-2xl max-w-md w-full mx-4 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="w-10 h-10 rounded-full bg-green-500/20 flex items-center justify-center">
          <svg class="w-5 h-5 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/></svg>
        </div>
        <h3 class="text-lg font-semibold text-green-400">Restore from Snapshot</h3>
      </div>
      <div class="bg-slate-800/50 rounded-lg p-3 mb-4 text-xs text-slate-400 space-y-1">
        <div><span class="text-slate-500">Snapshot:</span> <span class="text-slate-200">${esc(name)}</span></div>
        <div><span class="text-slate-500">Provider:</span> <span class="text-slate-200">${esc(provider)}</span></div>
      </div>
      <p class="text-sm text-slate-300 mb-3">This will create a <strong class="text-green-400">new instance</strong> from this snapshot.</p>
      ${credsHtml}
      <div id="restore-modal-status" class="text-sm mt-3 hidden"></div>
      <div class="flex gap-3 justify-end mt-4">
        <button onclick="closeRestoreModal()" class="px-4 py-2 text-sm font-medium text-slate-300 bg-slate-800 hover:bg-slate-700 border border-slate-600 rounded-lg transition-colors">Cancel</button>
        <button id="restore-modal-btn" onclick="confirmRestore('${esc(name)}','${esc(provider)}')" class="px-4 py-2 text-sm font-medium text-white bg-green-600 hover:bg-green-500 rounded-lg transition-colors">Restore Instance</button>
      </div>
    </div>
  `;
  document.body.appendChild(modal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeRestoreModal(); });
}

function closeRestoreModal() {
  const modal = document.getElementById('restore-modal');
  if (modal) modal.remove();
}

async function confirmRestore(snapshotName, provider) {
  const btn = document.getElementById('restore-modal-btn');
  const status = document.getElementById('restore-modal-status');

  btn.disabled = true;
  btn.textContent = 'Restoring...';
  btn.className = 'px-4 py-2 text-sm font-medium text-slate-400 bg-slate-700 rounded-lg cursor-not-allowed';
  status.className = 'text-sm mt-3 text-yellow-400';
  status.innerHTML = '<div class="flex items-center gap-2"><svg class="animate-spin h-4 w-4" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" fill="none"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path></svg>Restoring from snapshot... this may take 5-10 minutes.</div>';
  status.classList.remove('hidden');

  const body = { snapshot_name: snapshotName, provider: provider };
  if (provider === 'digitalocean') {
    body.do_token = (document.getElementById('restore-do-token') || {}).value || '';
    body.aws_region = (document.getElementById('restore-do-region') || {}).value || 'sgp1';
  } else if (provider === 'lightsail') {
    body.aws_region = (document.getElementById('restore-aws-region') || {}).value || 'ap-southeast-1';
  } else if (provider === 'byteplus') {
    body.byteplus_access_key = (document.getElementById('restore-bp-ak') || {}).value || '';
    body.byteplus_secret_key = (document.getElementById('restore-bp-sk') || {}).value || '';
    body.byteplus_region = (document.getElementById('restore-bp-region') || {}).value || 'ap-southeast-1';
    body.spot = (document.getElementById('restore-bp-spot') || {}).checked || false;
  }

  try {
    const res = await fetch('/api/snapshots/restore', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const data = await res.json();
    if (data.ok) {
      status.className = 'text-sm mt-3 text-green-400';
      status.textContent = data.message;
      btn.textContent = 'Done';
      setTimeout(() => { closeRestoreModal(); loadDeployments(); }, 2000);
    } else {
      status.className = 'text-sm mt-3 text-red-400';
      status.textContent = data.message || 'Restore failed.';
      btn.disabled = false;
      btn.textContent = 'Restore Instance';
      btn.className = 'px-4 py-2 text-sm font-medium text-white bg-green-600 hover:bg-green-500 rounded-lg transition-colors';
    }
  } catch (e) {
    status.className = 'text-sm mt-3 text-red-400';
    status.textContent = 'Network error: ' + e.message;
    btn.disabled = false;
    btn.textContent = 'Restore Instance';
    btn.className = 'px-4 py-2 text-sm font-medium text-white bg-green-600 hover:bg-green-500 rounded-lg transition-colors';
  }
}

function depSnapshot(id, provider, hostname) {
  const existing = document.getElementById('snapshot-modal');
  if (existing) existing.remove();

  const defaultName = hostname + '-' + new Date().toISOString().slice(0,10);
  let credsHtml = '';
  if (provider === 'digitalocean') {
    credsHtml = '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">DigitalOcean Token <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="snap-modal-do-token" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-cyan-500 focus:border-transparent" placeholder="dop_v1_...">' + eyeBtn() + '</div></div>';
  } else if (provider === 'lightsail') {
    credsHtml = '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">AWS Region</label><input type="text" id="snap-modal-aws-region" value="ap-southeast-1" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 focus:ring-2 focus:ring-cyan-500 focus:border-transparent"></div>';
  } else if (provider === 'byteplus') {
    credsHtml = '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">BytePlus Access Key <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="snap-modal-bp-ak" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-cyan-500 focus:border-transparent" placeholder="AKLT...">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">BytePlus Secret Key <span class="text-red-400">*</span></label><div class="relative"><input type="password" id="snap-modal-bp-sk" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 placeholder-slate-500 focus:ring-2 focus:ring-cyan-500 focus:border-transparent">' + eyeBtn() + '</div></div>' +
      '<div class="mt-3"><label class="block text-sm font-medium text-slate-300 mb-1">Region</label><input type="text" id="snap-modal-bp-region" value="ap-southeast-1" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 focus:ring-2 focus:ring-cyan-500 focus:border-transparent"></div>';
  } else {
    credsHtml = '<p class="text-slate-400 text-sm mt-3">Snapshot not supported for provider "' + esc(provider) + '".</p>';
  }

  const modal = document.createElement('div');
  modal.id = 'snapshot-modal';
  modal.className = 'fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm';
  modal.innerHTML = `
    <div class="bg-slate-900 border border-slate-700 rounded-xl shadow-2xl max-w-md w-full mx-4 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="w-10 h-10 rounded-full bg-cyan-500/20 flex items-center justify-center">
          <svg class="w-5 h-5 text-cyan-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"/></svg>
        </div>
        <h3 class="text-lg font-semibold text-cyan-400">Create Snapshot</h3>
      </div>
      <div class="bg-slate-800/50 rounded-lg p-3 mb-4 text-xs text-slate-400 space-y-1">
        <div><span class="text-slate-500">Hostname:</span> <span class="text-slate-200">${esc(hostname || '-')}</span></div>
        <div><span class="text-slate-500">Provider:</span> <span class="text-slate-200">${esc(provider || '-')}</span></div>
      </div>
      <div>
        <label class="block text-sm font-medium text-slate-300 mb-1">Snapshot Name <span class="text-red-400">*</span></label>
        <input type="text" id="snap-modal-name" value="${esc(defaultName)}" class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-sm text-slate-200 focus:ring-2 focus:ring-cyan-500 focus:border-transparent">
      </div>
      ${credsHtml}
      <div id="snap-modal-status" class="text-sm mt-3 hidden"></div>
      <div class="flex gap-3 justify-end mt-4">
        <button onclick="closeSnapshotModal()" class="px-4 py-2 text-sm font-medium text-slate-300 bg-slate-800 hover:bg-slate-700 border border-slate-600 rounded-lg transition-colors">Cancel</button>
        <button id="snap-modal-btn" onclick="confirmSnapshot('${esc(id)}','${esc(provider)}')" class="px-4 py-2 text-sm font-medium text-white bg-cyan-600 hover:bg-cyan-500 rounded-lg transition-colors">Create Snapshot</button>
      </div>
    </div>
  `;
  document.body.appendChild(modal);
  modal.addEventListener('click', (e) => { if (e.target === modal) closeSnapshotModal(); });
}

function closeSnapshotModal() {
  const modal = document.getElementById('snapshot-modal');
  if (modal) modal.remove();
}

async function confirmSnapshot(id, provider) {
  const btn = document.getElementById('snap-modal-btn');
  const status = document.getElementById('snap-modal-status');
  const snapshotName = (document.getElementById('snap-modal-name') || {}).value || '';
  if (!snapshotName) { status.className = 'text-sm mt-3 text-red-400'; status.textContent = 'Snapshot name is required.'; status.classList.remove('hidden'); return; }

  btn.disabled = true;
  btn.textContent = 'Creating...';
  btn.className = 'px-4 py-2 text-sm font-medium text-slate-400 bg-slate-700 rounded-lg cursor-not-allowed';
  status.className = 'text-sm mt-3 text-yellow-400';
  status.innerHTML = '<div class="flex items-center gap-2"><svg class="animate-spin h-4 w-4" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" fill="none"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path></svg>Creating snapshot... this may take several minutes.</div>';
  status.classList.remove('hidden');

  const body = { snapshot_name: snapshotName };
  if (provider === 'digitalocean') {
    body.do_token = (document.getElementById('snap-modal-do-token') || {}).value || '';
  } else if (provider === 'lightsail') {
    body.aws_region = (document.getElementById('snap-modal-aws-region') || {}).value || 'ap-southeast-1';
  } else if (provider === 'byteplus') {
    body.byteplus_access_key = (document.getElementById('snap-modal-bp-ak') || {}).value || '';
    body.byteplus_secret_key = (document.getElementById('snap-modal-bp-sk') || {}).value || '';
    body.byteplus_region = (document.getElementById('snap-modal-bp-region') || {}).value || 'ap-southeast-1';
  }

  try {
    const res = await fetch('/api/deployments/' + encodeURIComponent(id) + '/snapshot', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const data = await res.json();
    if (data.ok) {
      status.className = 'text-sm mt-3 text-green-400';
      status.textContent = data.message;
      btn.textContent = 'Done';
      setTimeout(() => { closeSnapshotModal(); }, 2000);
    } else {
      status.className = 'text-sm mt-3 text-red-400';
      status.textContent = data.message || 'Snapshot failed.';
      btn.disabled = false;
      btn.textContent = 'Create Snapshot';
      btn.className = 'px-4 py-2 text-sm font-medium text-white bg-cyan-600 hover:bg-cyan-500 rounded-lg transition-colors';
    }
  } catch (e) {
    status.className = 'text-sm mt-3 text-red-400';
    status.textContent = 'Network error: ' + e.message;
    btn.disabled = false;
    btn.textContent = 'Create Snapshot';
    btn.className = 'px-4 py-2 text-sm font-medium text-white bg-cyan-600 hover:bg-cyan-500 rounded-lg transition-colors';
  }
}
</script>
</body>
</html>
"##;
