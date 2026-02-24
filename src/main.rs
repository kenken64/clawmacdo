mod cloud_init;
mod commands;
mod config;
mod digitalocean;
mod error;
mod ssh;
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

    /// Full 1-click deploy: SSH keys → DO droplet → install OpenClaw + Claude Code + Codex → restore config
    Deploy {
        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: String,

        /// Anthropic API key (written to server .env)
        #[arg(long, env = "ANTHROPIC_API_KEY")]
        anthropic_key: String,

        /// OpenAI API key (written to server .env)
        #[arg(long, env = "OPENAI_API_KEY", default_value = "")]
        openai_key: String,

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
    },

    /// DO → DO migration: backup source droplet, deploy new, restore config
    Migrate {
        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: String,

        /// Anthropic API key (written to server .env)
        #[arg(long, env = "ANTHROPIC_API_KEY")]
        anthropic_key: String,

        /// OpenAI API key (written to server .env)
        #[arg(long, env = "OPENAI_API_KEY", default_value = "")]
        openai_key: String,

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
    },

    /// List deployed openclaw-tagged droplets
    Status {
        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: String,
    },

    /// Destroy an openclaw-tagged droplet by name and clean up SSH keys
    Destroy {
        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: String,

        /// Droplet name
        #[arg(long)]
        name: String,
    },

    /// Show local backup archives with sizes and dates
    ListBackups,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Backup => {
            commands::backup::run()?;
        }
        Commands::Deploy {
            do_token,
            anthropic_key,
            openai_key,
            region,
            size,
            hostname,
            backup,
            enable_backups,
        } => {
            let params = DeployParams {
                do_token,
                anthropic_key,
                openai_key,
                region,
                size,
                hostname,
                backup,
                enable_backups,
                non_interactive: false,
            };
            commands::deploy::run(params).await?;
        }
        Commands::Migrate {
            do_token,
            anthropic_key,
            openai_key,
            source_ip,
            source_key,
            region,
            size,
            hostname,
        } => {
            let params = MigrateParams {
                do_token,
                anthropic_key,
                openai_key,
                source_ip,
                source_key,
                region,
                size,
                hostname,
            };
            commands::migrate::run(params).await?;
        }
        Commands::Status { do_token } => {
            commands::status::run(&do_token).await?;
        }
        Commands::Destroy { do_token, name } => {
            let params = DestroyParams { do_token, name };
            commands::destroy::run(params).await?;
        }
        Commands::ListBackups => {
            commands::list_backups::run()?;
        }
    }

    Ok(())
}
