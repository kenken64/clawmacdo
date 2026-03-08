mod commands;

use clap::{Parser, Subcommand};
use commands::deploy::DeployParams;
use commands::destroy::DestroyParams;
use commands::migrate::MigrateParams;
use std::path::PathBuf;

// Use the new crate structure
use clawmacdo_core::*;
use clawmacdo_cloud::*;
use clawmacdo_db::*;
use clawmacdo_provision::*;
use clawmacdo_ssh::*;
use clawmacdo_ui::*;

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

        /// Provider (digitalocean or tencent)
        #[arg(long, env = "PROVIDER", default_value = "digitalocean")]
        provider: String,

        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: Option<String>,

        /// Tencent Cloud Secret ID
        #[arg(long, env = "TENCENT_SECRET_ID")]
        tencent_secret_id: Option<String>,

        /// Tencent Cloud Secret Key
        #[arg(long, env = "TENCENT_SECRET_KEY")]
        tencent_secret_key: Option<String>,

        /// Anthropic API key or setup token
        #[arg(long, env = "ANTHROPIC_API_KEY")]
        anthropic_key: Option<String>,

        /// OpenAI API key (optional)
        #[arg(long, env = "OPENAI_API_KEY")]
        openai_key: Option<String>,

        /// Gemini API key (optional)
        #[arg(long, env = "GEMINI_API_KEY")]
        gemini_key: Option<String>,

        /// WhatsApp phone number (optional)
        #[arg(long, env = "WHATSAPP_PHONE_NUMBER")]
        whatsapp_phone_number: Option<String>,

        /// Telegram bot token (optional)
        #[arg(long, env = "TELEGRAM_BOT_TOKEN")]
        telegram_bot_token: Option<String>,

        /// Tailscale auth key (optional, used with --tailscale)
        #[arg(long, env = "TAILSCALE_AUTH_KEY")]
        tailscale_auth_key: Option<String>,

        /// Region to deploy to
        #[arg(long, default_value = "sgp1")]
        region: String,

        /// Instance size
        #[arg(long, default_value = "s-2vcpu-4gb")]
        size: String,

        /// Custom hostname (optional)
        #[arg(long)]
        hostname: Option<String>,

        /// Path to backup file to restore (optional)
        #[arg(long)]
        backup: Option<PathBuf>,

        /// Enable automatic backups
        #[arg(long)]
        enable_backups: bool,

        /// Enable Tailscale VPN
        #[arg(long)]
        tailscale: bool,

        /// Enable OpenClaw sandbox environment
        #[arg(long)]
        enable_sandbox: bool,
    },

    /// Destroy an instance by name and clean up SSH keys
    Destroy {
        /// Provider (digitalocean or tencent)
        #[arg(long, env = "PROVIDER", default_value = "digitalocean")]
        provider: String,

        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: Option<String>,

        /// Tencent Cloud Secret ID
        #[arg(long, env = "TENCENT_SECRET_ID")]
        tencent_secret_id: Option<String>,

        /// Tencent Cloud Secret Key
        #[arg(long, env = "TENCENT_SECRET_KEY")]
        tencent_secret_key: Option<String>,

        /// Instance name to destroy
        #[arg(long)]
        name: String,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,

        /// Force destruction (alias for --yes)
        #[arg(long)]
        force: bool,
    },

    /// Cloud-to-cloud migration: backup source, deploy new, restore
    Migrate {
        /// Customer name (who is migrating)
        #[arg(long)]
        customer_name: String,

        /// Provider (digitalocean or tencent)
        #[arg(long, env = "PROVIDER", default_value = "digitalocean")]
        provider: String,

        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: Option<String>,

        /// Tencent Cloud Secret ID
        #[arg(long, env = "TENCENT_SECRET_ID")]
        tencent_secret_id: Option<String>,

        /// Tencent Cloud Secret Key
        #[arg(long, env = "TENCENT_SECRET_KEY")]
        tencent_secret_key: Option<String>,

        /// Anthropic API key or setup token
        #[arg(long, env = "ANTHROPIC_API_KEY")]
        anthropic_key: Option<String>,

        /// OpenAI API key (optional)
        #[arg(long, env = "OPENAI_API_KEY")]
        openai_key: Option<String>,

        /// Gemini API key (optional)
        #[arg(long, env = "GEMINI_API_KEY")]
        gemini_key: Option<String>,

        /// WhatsApp phone number (optional)
        #[arg(long, env = "WHATSAPP_PHONE_NUMBER")]
        whatsapp_phone_number: Option<String>,

        /// Telegram bot token (optional)
        #[arg(long, env = "TELEGRAM_BOT_TOKEN")]
        telegram_bot_token: Option<String>,

        /// Source instance IP address
        #[arg(long)]
        source_ip: String,

        /// Source SSH private key path
        #[arg(long)]
        source_key: PathBuf,

        /// Region to deploy to
        #[arg(long, default_value = "sgp1")]
        region: String,

        /// Instance size
        #[arg(long, default_value = "s-2vcpu-4gb")]
        size: String,

        /// Custom hostname (optional)
        #[arg(long)]
        hostname: Option<String>,

        /// Enable automatic backups
        #[arg(long)]
        enable_backups: bool,

        /// Enable Tailscale VPN
        #[arg(long)]
        tailscale: bool,

        /// Tailscale auth key (optional, used with --tailscale)
        #[arg(long, env = "TAILSCALE_AUTH_KEY")]
        tailscale_auth_key: Option<String>,

        /// Enable OpenClaw sandbox environment
        #[arg(long)]
        enable_sandbox: bool,
    },

    /// List deployed openclaw-tagged instances
    Status {
        /// Provider (digitalocean or tencent)
        #[arg(long, env = "PROVIDER", default_value = "digitalocean")]
        provider: String,

        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: Option<String>,

        /// Tencent Cloud Secret ID
        #[arg(long, env = "TENCENT_SECRET_ID")]
        tencent_secret_id: Option<String>,

        /// Tencent Cloud Secret Key
        #[arg(long, env = "TENCENT_SECRET_KEY")]
        tencent_secret_key: Option<String>,
    },

    /// Show local backup archives with sizes and dates
    ListBackups,

    /// Start the local web UI
    Serve {
        /// Port to serve on
        #[arg(long, default_value = "3456")]
        port: u16,
    },

    /// Repair WhatsApp channel support on an existing instance
    WhatsappRepair {
        /// Target instance IP address
        #[arg(long)]
        ip: String,

        /// SSH private key path
        #[arg(long)]
        ssh_key_path: PathBuf,
    },

    /// Repair agent Docker access on an existing instance
    DockerFix {
        /// Target instance IP address
        #[arg(long)]
        ip: String,

        /// SSH private key path
        #[arg(long)]
        ssh_key_path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok(); // Load .env file if present
    let cli = Cli::parse();

    match cli.command {
        Commands::Backup => commands::backup::run().await,
        Commands::Deploy {
            customer_name,
            provider,
            do_token,
            tencent_secret_id,
            tencent_secret_key,
            anthropic_key,
            openai_key,
            gemini_key,
            whatsapp_phone_number,
            telegram_bot_token,
            tailscale_auth_key,
            region,
            size,
            hostname,
            backup,
            enable_backups,
            tailscale,
            enable_sandbox,
        } => {
            let params = DeployParams {
                customer_name,
                provider,
                do_token,
                tencent_secret_id,
                tencent_secret_key,
                anthropic_key,
                openai_key,
                gemini_key,
                whatsapp_phone_number,
                telegram_bot_token,
                tailscale_auth_key,
                region,
                size,
                hostname,
                backup,
                enable_backups,
                tailscale,
                enable_sandbox,
            };
            commands::deploy::run(params).await
        }
        Commands::Destroy {
            provider,
            do_token,
            tencent_secret_id,
            tencent_secret_key,
            name,
            yes,
            force,
        } => {
            let params = DestroyParams {
                provider,
                do_token,
                tencent_secret_id,
                tencent_secret_key,
                name,
                yes: yes || force,
            };
            commands::destroy::run(params).await
        }
        Commands::Migrate {
            customer_name,
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
            enable_backups,
            tailscale,
            tailscale_auth_key,
            enable_sandbox,
        } => {
            let params = MigrateParams {
                customer_name,
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
                enable_backups,
                tailscale,
                tailscale_auth_key,
                enable_sandbox,
            };
            commands::migrate::run(params).await
        }
        Commands::Status {
            provider,
            do_token,
            tencent_secret_id,
            tencent_secret_key,
        } => {
            commands::status::run(provider, do_token, tencent_secret_id, tencent_secret_key).await
        }
        Commands::ListBackups => commands::list_backups::run().await,
        Commands::Serve { port } => commands::serve::run(port).await,
        Commands::WhatsappRepair { ip, ssh_key_path } => {
            commands::whatsapp::run(ip, ssh_key_path).await
        }
        Commands::DockerFix { ip, ssh_key_path } => {
            commands::docker_fix::run(ip, ssh_key_path).await
        }
    }
}