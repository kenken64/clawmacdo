mod cloud_init;
mod cloud_provider;
mod commands;
mod config;
mod db;
mod digitalocean;
mod error;
mod progress;
pub mod provision;
mod ssh;
mod tencent;
mod ui;

use clap::{Parser, Subcommand};
use commands::deploy::DeployParams;
use commands::destroy::DestroyParams;
use commands::migrate::MigrateParams;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "clawmacdo",
    version,
    about = "CLI for migrating OpenClaw to DigitalOcean"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Archive ~/.openclaw/ and LaunchAgent plist into a timestamped .tar.gz
    Backup,

    /// Full 1-click deploy: SSH keys → cloud instance → install OpenClaw + Claude Code + Codex → restore config
    Deploy {
        /// Customer name (who is deploying)
        #[arg(long)]
        customer_name: String,

        /// Customer email
        #[arg(long)]
        customer_email: String,

        /// Cloud provider: digitalocean or tencent
        #[arg(long, default_value = "digitalocean")]
        provider: String,

        /// DigitalOcean API token (required for digitalocean provider)
        #[arg(long, env = "DO_TOKEN", default_value = "")]
        do_token: String,

        /// Tencent Cloud SecretId (required for tencent provider)
        #[arg(long, env = "TENCENT_SECRET_ID", default_value = "")]
        tencent_secret_id: String,

        /// Tencent Cloud SecretKey (required for tencent provider)
        #[arg(long, env = "TENCENT_SECRET_KEY", default_value = "")]
        tencent_secret_key: String,

        /// Anthropic API key or setup token (sk-ant-api... or sk-ant-oat...)
        #[arg(long, env = "ANTHROPIC_API_KEY")]
        anthropic_key: String,

        /// OpenAI API key (written to server .env)
        #[arg(long, env = "OPENAI_API_KEY", default_value = "")]
        openai_key: String,

        /// Google Gemini API key (written to server .env)
        #[arg(long, env = "GEMINI_API_KEY", default_value = "")]
        gemini_key: String,

        /// WhatsApp phone number (written to server .env)
        #[arg(long, env = "WHATSAPP_PHONE_NUMBER", default_value = "")]
        whatsapp_phone_number: String,

        /// Telegram bot token (written to server .env)
        #[arg(long, env = "TELEGRAM_BOT_TOKEN", default_value = "")]
        telegram_bot_token: String,

        /// DigitalOcean region slug (e.g. sgp1, nyc1)
        #[arg(long)]
        region: Option<String>,

        /// Droplet size slug (e.g. s-2vcpu-4gb)
        #[arg(long)]
        size: Option<String>,

        /// Droplet hostname
        #[arg(long)]
        hostname: Option<String>,

        /// Path to a specific backup archive to restore
        #[arg(long)]
        backup: Option<PathBuf>,

        /// Enable DigitalOcean automated backups
        #[arg(long, default_value = "false")]
        enable_backups: bool,

        /// Enable OpenClaw sandbox mode (Docker-based tool isolation)
        #[arg(long, default_value = "false")]
        enable_sandbox: bool,

        /// Enable Tailscale VPN on the droplet
        #[arg(long, default_value = "false")]
        tailscale: bool,

        /// Tailscale auth key for automatic `tailscale up` (optional)
        #[arg(long, env = "TAILSCALE_AUTH_KEY")]
        tailscale_auth_key: Option<String>,
    },

    /// Cloud-to-cloud migration: backup source instance, deploy new, restore config
    Migrate {
        /// Cloud provider: digitalocean or tencent
        #[arg(long, default_value = "digitalocean")]
        provider: String,

        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN", default_value = "")]
        do_token: String,

        /// Tencent Cloud SecretId
        #[arg(long, env = "TENCENT_SECRET_ID", default_value = "")]
        tencent_secret_id: String,

        /// Tencent Cloud SecretKey
        #[arg(long, env = "TENCENT_SECRET_KEY", default_value = "")]
        tencent_secret_key: String,

        /// Anthropic API key or setup token (sk-ant-api... or sk-ant-oat...)
        #[arg(long, env = "ANTHROPIC_API_KEY")]
        anthropic_key: String,

        /// OpenAI API key (written to server .env)
        #[arg(long, env = "OPENAI_API_KEY", default_value = "")]
        openai_key: String,

        /// Google Gemini API key (written to server .env)
        #[arg(long, env = "GEMINI_API_KEY", default_value = "")]
        gemini_key: String,

        /// WhatsApp phone number (written to server .env)
        #[arg(long, env = "WHATSAPP_PHONE_NUMBER", default_value = "")]
        whatsapp_phone_number: String,

        /// Telegram bot token (written to server .env)
        #[arg(long, env = "TELEGRAM_BOT_TOKEN", default_value = "")]
        telegram_bot_token: String,

        /// IP address of the source droplet
        #[arg(long)]
        source_ip: String,

        /// Path to SSH private key for the source droplet
        #[arg(long)]
        source_key: PathBuf,

        /// DigitalOcean region for the new droplet
        #[arg(long)]
        region: Option<String>,

        /// Droplet size for the new droplet
        #[arg(long)]
        size: Option<String>,

        /// Hostname for the new droplet
        #[arg(long)]
        hostname: Option<String>,

        /// Enable OpenClaw sandbox mode (Docker-based tool isolation)
        #[arg(long, default_value = "false")]
        enable_sandbox: bool,

        /// Enable Tailscale VPN on the droplet
        #[arg(long, default_value = "false")]
        tailscale: bool,

        /// Tailscale auth key for automatic `tailscale up` (optional)
        #[arg(long, env = "TAILSCALE_AUTH_KEY")]
        tailscale_auth_key: Option<String>,
    },

    /// List deployed openclaw-tagged instances
    Status {
        /// Cloud provider: digitalocean or tencent
        #[arg(long, default_value = "digitalocean")]
        provider: String,

        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN", default_value = "")]
        do_token: String,

        /// Tencent Cloud SecretId
        #[arg(long, env = "TENCENT_SECRET_ID", default_value = "")]
        tencent_secret_id: String,

        /// Tencent Cloud SecretKey
        #[arg(long, env = "TENCENT_SECRET_KEY", default_value = "")]
        tencent_secret_key: String,
    },

    /// Destroy an openclaw-tagged instance by name and clean up SSH keys
    Destroy {
        /// Cloud provider: digitalocean or tencent
        #[arg(long, default_value = "digitalocean")]
        provider: String,

        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN", default_value = "")]
        do_token: String,

        /// Tencent Cloud SecretId
        #[arg(long, env = "TENCENT_SECRET_ID", default_value = "")]
        tencent_secret_id: String,

        /// Tencent Cloud SecretKey
        #[arg(long, env = "TENCENT_SECRET_KEY", default_value = "")]
        tencent_secret_key: String,

        /// Instance name
        #[arg(long)]
        name: String,

        /// Skip confirmation prompt
        #[arg(long, alias = "force")]
        yes: bool,
    },

    /// Show local backup archives with sizes and dates
    ListBackups,

    /// Launch a local web UI for deploying OpenClaw
    Serve {
        /// Port for the web server
        #[arg(long, default_value = "3456")]
        port: u16,
    },

    /// Repair WhatsApp channel support on an existing droplet (update OpenClaw + restart gateway)
    WhatsappRepair {
        /// Droplet IP address
        #[arg(long)]
        ip: String,

        /// Path to SSH private key for the target droplet
        #[arg(long)]
        ssh_key_path: PathBuf,
    },

    /// Repair agent Docker access on an existing droplet (gateway service + docker socket perms path)
    DockerFix {
        /// Droplet IP address
        #[arg(long)]
        ip: String,

        /// Path to SSH private key for the target droplet
        #[arg(long)]
        ssh_key_path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    match cli.command {
        Commands::Backup => {
            commands::backup::run()?;
        }
        Commands::Deploy {
            customer_name,
            customer_email,
            provider,
            do_token,
            tencent_secret_id,
            tencent_secret_key,
            anthropic_key,
            openai_key,
            gemini_key,
            whatsapp_phone_number,
            telegram_bot_token,
            region,
            size,
            hostname,
            backup,
            enable_backups,
            enable_sandbox,
            tailscale,
            tailscale_auth_key,
        } => {
            let params = DeployParams {
                customer_name,
                customer_email,
                provider,
                do_token,
                tencent_secret_id,
                tencent_secret_key,
                anthropic_key,
                openai_key,
                gemini_key,
                whatsapp_phone_number,
                telegram_bot_token,
                region,
                size,
                hostname,
                backup,
                enable_backups,
                enable_sandbox,
                tailscale,
                tailscale_auth_key,
                non_interactive: false,
                progress_tx: None,
            };
            commands::deploy::run(params).await?;
        }
        Commands::Migrate {
            provider,
            do_token,
            tencent_secret_id,
            tencent_secret_key,
            anthropic_key,
            openai_key,
            gemini_key,
            whatsapp_phone_number,
            telegram_bot_token,
            source_ip,
            source_key,
            region,
            size,
            hostname,
            enable_sandbox,
            tailscale,
            tailscale_auth_key,
        } => {
            let params = MigrateParams {
                provider,
                do_token,
                tencent_secret_id,
                tencent_secret_key,
                anthropic_key,
                openai_key,
                gemini_key,
                whatsapp_phone_number,
                telegram_bot_token,
                source_ip,
                source_key,
                region,
                size,
                hostname,
                enable_sandbox,
                tailscale,
                tailscale_auth_key,
            };
            commands::migrate::run(params).await?;
        }
        Commands::Status {
            provider,
            do_token,
            tencent_secret_id,
            tencent_secret_key,
        } => {
            commands::status::run(&provider, &do_token, &tencent_secret_id, &tencent_secret_key)
                .await?;
        }
        Commands::Destroy {
            provider,
            do_token,
            tencent_secret_id,
            tencent_secret_key,
            name,
            yes,
        } => {
            let params = DestroyParams {
                provider,
                do_token,
                tencent_secret_id,
                tencent_secret_key,
                name,
                yes,
            };
            commands::destroy::run(params).await?;
        }
        Commands::ListBackups => {
            commands::list_backups::run()?;
        }
        Commands::Serve { port } => {
            commands::serve::run(port).await?;
        }
        Commands::WhatsappRepair { ip, ssh_key_path } => {
            commands::whatsapp::run(&ip, &ssh_key_path).await?;
        }
        Commands::DockerFix { ip, ssh_key_path } => {
            commands::docker_fix::run(&ip, &ssh_key_path).await?;
        }
    }

    Ok(())
}
