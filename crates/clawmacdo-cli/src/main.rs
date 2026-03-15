mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "clawmacdo",
    version,
    about = "Deploy and manage OpenClaw instances"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Track a deployment's progress
    Track {
        /// Deploy ID, hostname, or IP address
        query: String,
        /// Follow mode — poll and refresh until deployment finishes
        #[arg(short, long)]
        follow: bool,
        /// Output as NDJSON instead of human-readable
        #[arg(long)]
        json: bool,
    },
    /// Destroy a deployed instance
    Destroy {
        /// Cloud provider (digitalocean, tencent, lightsail, azure, byteplus)
        #[arg(long)]
        provider: String,
        /// Instance name or ID to destroy (empty = list all)
        #[arg(long, default_value = "")]
        name: String,
        /// DigitalOcean API token
        #[arg(long, default_value = "", env = "DO_TOKEN")]
        do_token: String,
        /// Tencent SecretId
        #[arg(long, default_value = "", env = "TENCENT_SECRET_ID")]
        tencent_secret_id: String,
        /// Tencent SecretKey
        #[arg(long, default_value = "", env = "TENCENT_SECRET_KEY")]
        tencent_secret_key: String,
        /// AWS region (Lightsail)
        #[arg(long, default_value = "ap-southeast-1")]
        aws_region: String,
        /// Azure Tenant ID
        #[arg(long, default_value = "")]
        azure_tenant_id: String,
        /// Azure Subscription ID
        #[arg(long, default_value = "")]
        azure_subscription_id: String,
        /// Azure Client ID
        #[arg(long, default_value = "")]
        azure_client_id: String,
        /// Azure Client Secret
        #[arg(long, default_value = "")]
        azure_client_secret: String,
        /// Azure Resource Group
        #[arg(long, default_value = "")]
        azure_resource_group: String,
        /// BytePlus Access Key
        #[arg(long, default_value = "", env = "BYTEPLUS_ACCESS_KEY")]
        byteplus_access_key: String,
        /// BytePlus Secret Key
        #[arg(long, default_value = "", env = "BYTEPLUS_SECRET_KEY")]
        byteplus_secret_key: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
    /// Start the web UI server
    #[cfg(feature = "web-ui")]
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "3456")]
        port: u16,
    },
}

/// Check that required external CLIs are installed for compiled-in providers.
/// Auto-installs missing CLIs when possible (brew on macOS, official scripts on Linux).
fn preflight_cli_checks() {
    #[cfg(feature = "lightsail")]
    {
        if let Err(e) = clawmacdo_cloud::lightsail_cli::ensure_aws_cli() {
            eprintln!("Warning: AWS CLI prerequisite check failed: {e}");
        }
    }
    #[cfg(feature = "azure")]
    {
        if let Err(e) = clawmacdo_cloud::azure_cli::ensure_az_cli() {
            eprintln!("Warning: Azure CLI prerequisite check failed: {e}");
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    preflight_cli_checks();

    let cli = Cli::parse();

    match cli.command {
        Commands::Track {
            query,
            follow,
            json,
        } => {
            commands::track::run(commands::track::TrackParams {
                query,
                follow,
                json,
            })
            .await
        }
        Commands::Destroy {
            provider,
            name,
            do_token,
            tencent_secret_id,
            tencent_secret_key,
            aws_region,
            azure_tenant_id,
            azure_subscription_id,
            azure_client_id,
            azure_client_secret,
            azure_resource_group,
            byteplus_access_key,
            byteplus_secret_key,
            yes,
        } => {
            commands::destroy::run(commands::destroy::DestroyParams {
                provider,
                do_token,
                tencent_secret_id,
                tencent_secret_key,
                aws_region,
                azure_tenant_id,
                azure_subscription_id,
                azure_client_id,
                azure_client_secret,
                azure_resource_group,
                byteplus_access_key,
                byteplus_secret_key,
                name,
                yes,
            })
            .await
        }
        #[cfg(feature = "web-ui")]
        Commands::Serve { port } => commands::serve::run(port).await,
    }
}
