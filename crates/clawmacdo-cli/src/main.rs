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
    /// Start the web UI server
    #[cfg(feature = "web-ui")]
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "3456")]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
        #[cfg(feature = "web-ui")]
        Commands::Serve { port } => commands::serve::run(port).await,
    }
}
