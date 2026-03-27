use anyhow::Result;
use clawmacdo_db as db;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::commands::deploy::{self, DeployParams};

pub struct DeployCmdArgs {
    pub provider: String,
    pub customer_name: String,
    pub customer_email: String,
    pub do_token: String,
    pub tencent_secret_id: String,
    pub tencent_secret_key: String,
    pub aws_access_key_id: String,
    pub aws_secret_access_key: String,
    pub aws_region: String,
    pub azure_tenant_id: String,
    pub azure_subscription_id: String,
    pub azure_client_id: String,
    pub azure_client_secret: String,
    pub byteplus_access_key: String,
    pub byteplus_secret_key: String,
    pub byteplus_ark_api_key: String,
    pub anthropic_key: String,
    pub openai_key: String,
    pub gemini_key: String,
    pub whatsapp_phone_number: String,
    pub telegram_bot_token: String,
    pub region: Option<String>,
    pub size: Option<String>,
    pub hostname: Option<String>,
    pub backup: Option<PathBuf>,
    pub enable_backups: bool,
    pub enable_sandbox: bool,
    pub tailscale: bool,
    pub tailscale_auth_key: String,
    pub primary_model: String,
    pub failover_1: String,
    pub failover_2: String,
    pub profile: String,
    pub spot: bool,
    pub openclaw_version: String,
    pub detach: bool,
    pub json: bool,
    /// Pre-assigned deploy ID (from detach re-exec)
    pub deploy_id: Option<String>,
}

pub async fn run(args: DeployCmdArgs) -> Result<()> {
    let conn = db::init_db()?;
    let db_handle: deploy::Db = Arc::new(Mutex::new(conn));

    let deploy_id = args
        .deploy_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Insert deployment row into SQLite (skip if re-exec child — parent already inserted)
    if args.deploy_id.is_none() {
        if let Ok(conn) = db_handle.lock() {
            db::insert_deployment(
                &conn,
                &deploy_id,
                &args.customer_name,
                &args.customer_email,
                &args.provider,
                args.region.as_deref().unwrap_or(""),
                args.size.as_deref().unwrap_or(""),
                args.hostname.as_deref().unwrap_or(""),
            )?;
        }
    }

    if args.detach {
        // Print deploy_id immediately
        if args.json {
            println!(
                "{}",
                serde_json::json!({
                    "event": "deploy_started",
                    "deploy_id": deploy_id,
                })
            );
        } else {
            println!("{deploy_id}");
        }

        // Re-exec self in background — the child runs in foreground mode
        // with the same deploy_id, writing progress to SQLite.
        let exe = std::env::current_exe()?;
        let mut child_args: Vec<String> = std::env::args().skip(1).collect();
        // Remove --detach and --json so child runs in foreground mode
        child_args.retain(|a| a != "--detach" && a != "--json");
        // Pass the pre-assigned deploy_id
        child_args.push(format!("--_deploy-id={deploy_id}"));

        // Log stdout+stderr to ~/.clawmacdo/deploy-<id>.log for debugging
        let log_path = clawmacdo_core::config::app_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(format!("deploy-{deploy_id}.log"));
        let log_file = std::fs::File::create(&log_path).ok();
        let (stdout_cfg, stderr_cfg) = match log_file {
            Some(f) => {
                let f2 = f
                    .try_clone()
                    .unwrap_or_else(|_| std::fs::File::create("/dev/null").expect("/dev/null"));
                (std::process::Stdio::from(f), std::process::Stdio::from(f2))
            }
            None => (std::process::Stdio::null(), std::process::Stdio::null()),
        };

        let mut cmd = std::process::Command::new(exe);
        cmd.args(&child_args)
            .stdin(std::process::Stdio::null())
            .stdout(stdout_cfg)
            .stderr(stderr_cfg);

        // Detach child from the controlling terminal so it survives parent exit
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }

        cmd.spawn()?;

        return Ok(());
    }

    // Foreground mode: run deploy to completion
    let params = DeployParams {
        deploy_id: Some(deploy_id.clone()),
        customer_name: args.customer_name,
        customer_email: args.customer_email,
        provider: args.provider,
        do_token: args.do_token,
        tencent_secret_id: args.tencent_secret_id,
        tencent_secret_key: args.tencent_secret_key,
        aws_access_key_id: args.aws_access_key_id,
        aws_secret_access_key: args.aws_secret_access_key,
        aws_region: args.aws_region,
        azure_tenant_id: args.azure_tenant_id,
        azure_subscription_id: args.azure_subscription_id,
        azure_client_id: args.azure_client_id,
        azure_client_secret: args.azure_client_secret,
        byteplus_access_key: args.byteplus_access_key,
        byteplus_secret_key: args.byteplus_secret_key,
        byteplus_ark_api_key: args.byteplus_ark_api_key,
        anthropic_key: args.anthropic_key,
        openai_key: args.openai_key,
        gemini_key: args.gemini_key,
        whatsapp_phone_number: args.whatsapp_phone_number,
        telegram_bot_token: args.telegram_bot_token,
        region: args.region,
        size: args.size,
        hostname: args.hostname,
        backup: args.backup,
        enable_backups: args.enable_backups,
        enable_sandbox: args.enable_sandbox,
        tailscale: args.tailscale,
        tailscale_auth_key: if args.tailscale_auth_key.trim().is_empty() {
            None
        } else {
            Some(args.tailscale_auth_key)
        },
        primary_model: args.primary_model,
        failover_1: args.failover_1,
        failover_2: args.failover_2,
        profile: args.profile,
        spot: args.spot,
        openclaw_version: args.openclaw_version,
        non_interactive: true,
        progress_tx: None,
        db: Some(db_handle.clone()),
    };

    let result = deploy::run(params).await;
    match result {
        Ok(record) => {
            if let Ok(conn) = db_handle.lock() {
                let _ = db::update_deployment_status(
                    &conn,
                    &deploy_id,
                    "completed",
                    Some(&record.ip_address),
                    Some(&record.hostname),
                );
            }
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "event": "deploy_complete",
                        "deploy_id": deploy_id,
                        "ip": record.ip_address,
                        "hostname": record.hostname,
                        "ssh_key_path": record.ssh_key_path,
                    })
                );
            } else {
                println!("Deploy complete!");
                println!("  ID:       {deploy_id}");
                println!("  IP:       {}", record.ip_address);
                println!("  Hostname: {}", record.hostname);
                println!(
                    "  SSH:      ssh -i {} root@{}",
                    record.ssh_key_path, record.ip_address
                );
            }
            Ok(())
        }
        Err(e) => {
            if let Ok(conn) = db_handle.lock() {
                let _ = db::update_deployment_status(&conn, &deploy_id, "failed", None, None);
            }
            Err(e)
        }
    }
}
