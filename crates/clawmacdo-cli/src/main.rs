mod commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    /// Deploy a new OpenClaw instance
    Deploy {
        /// Cloud provider (digitalocean, tencent, lightsail, azure, byteplus)
        #[arg(long)]
        provider: String,
        /// Customer name
        #[arg(long, default_value = "CLI User")]
        customer_name: String,
        /// Customer email
        #[arg(long)]
        customer_email: String,
        /// DigitalOcean API token
        #[arg(long, default_value = "", env = "DO_TOKEN")]
        do_token: String,
        /// Tencent SecretId
        #[arg(long, default_value = "", env = "TENCENT_SECRET_ID")]
        tencent_secret_id: String,
        /// Tencent SecretKey
        #[arg(long, default_value = "", env = "TENCENT_SECRET_KEY")]
        tencent_secret_key: String,
        /// AWS Access Key ID (Lightsail)
        #[arg(long, default_value = "", env = "AWS_ACCESS_KEY_ID")]
        aws_access_key_id: String,
        /// AWS Secret Access Key (Lightsail)
        #[arg(long, default_value = "", env = "AWS_SECRET_ACCESS_KEY")]
        aws_secret_access_key: String,
        /// AWS region (Lightsail)
        #[arg(long, default_value = "ap-southeast-1")]
        aws_region: String,
        /// Azure Tenant ID
        #[arg(long, default_value = "", env = "AZURE_TENANT_ID")]
        azure_tenant_id: String,
        /// Azure Subscription ID
        #[arg(long, default_value = "", env = "AZURE_SUBSCRIPTION_ID")]
        azure_subscription_id: String,
        /// Azure Client ID
        #[arg(long, default_value = "", env = "AZURE_CLIENT_ID")]
        azure_client_id: String,
        /// Azure Client Secret
        #[arg(long, default_value = "", env = "AZURE_CLIENT_SECRET")]
        azure_client_secret: String,
        /// BytePlus Access Key
        #[arg(long, default_value = "", env = "BYTEPLUS_ACCESS_KEY")]
        byteplus_access_key: String,
        /// BytePlus Secret Key
        #[arg(long, default_value = "", env = "BYTEPLUS_SECRET_KEY")]
        byteplus_secret_key: String,
        /// BytePlus Ark API Key
        #[arg(long, default_value = "", env = "BYTEPLUS_ARK_API_KEY")]
        byteplus_ark_api_key: String,
        /// Anthropic API key
        #[arg(long, default_value = "", env = "ANTHROPIC_API_KEY")]
        anthropic_key: String,
        /// OpenAI API key
        #[arg(long, default_value = "", env = "OPENAI_API_KEY")]
        openai_key: String,
        /// Gemini API key
        #[arg(long, default_value = "", env = "GEMINI_API_KEY")]
        gemini_key: String,
        /// WhatsApp phone number
        #[arg(long, default_value = "")]
        whatsapp_phone_number: String,
        /// Telegram bot token
        #[arg(long, default_value = "")]
        telegram_bot_token: String,
        /// Region override
        #[arg(long)]
        region: Option<String>,
        /// Instance size override
        #[arg(long)]
        size: Option<String>,
        /// Hostname
        #[arg(long)]
        hostname: Option<String>,
        /// Path to backup archive to restore
        #[arg(long)]
        backup: Option<PathBuf>,
        /// Enable provider backups
        #[arg(long)]
        enable_backups: bool,
        /// Enable sandbox mode
        #[arg(long)]
        enable_sandbox: bool,
        /// Enable Tailscale
        #[arg(long)]
        tailscale: bool,
        /// Tailscale auth key
        #[arg(long, default_value = "")]
        tailscale_auth_key: String,
        /// Primary AI model (anthropic, openai, gemini, byteplus)
        #[arg(long, default_value = "anthropic")]
        primary_model: String,
        /// First failover model
        #[arg(long, default_value = "")]
        failover_1: String,
        /// Second failover model
        #[arg(long, default_value = "")]
        failover_2: String,
        /// Profile (messaging, coding, full)
        #[arg(long, default_value = "full")]
        profile: String,
        /// Use spot instance for BytePlus (up to ~80% cheaper, may be reclaimed with 5 min warning)
        #[arg(long)]
        spot: bool,
        /// OpenClaw version to install (e.g. 2026.3.22). Use `openclaw-versions` to list available versions.
        #[arg(long)]
        openclaw_version: String,
        /// Detach: fork deploy to background, print deploy ID, exit immediately
        #[arg(long)]
        detach: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Internal: pre-assigned deploy ID (used by --detach re-exec)
        #[arg(long = "_deploy-id", hide = true)]
        _deploy_id: Option<String>,
    },
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
        /// AWS Access Key ID (Lightsail)
        #[arg(long, default_value = "", env = "AWS_ACCESS_KEY_ID")]
        aws_access_key_id: String,
        /// AWS Secret Access Key (Lightsail)
        #[arg(long, default_value = "", env = "AWS_SECRET_ACCESS_KEY")]
        aws_secret_access_key: String,
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
    /// Configure Telegram bot token on a deployed instance
    TelegramSetup {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Telegram bot token (from @BotFather)
        #[arg(long)]
        bot_token: String,
        /// Clear existing pairing state before setup (reset + setup in one SSH session)
        #[arg(long)]
        reset: bool,
    },
    /// Approve a Telegram pairing code to activate chat
    TelegramPair {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// 8-character pairing code from the bot
        #[arg(long)]
        code: String,
    },
    /// Retrieve the Telegram chat ID from a deployed instance
    TelegramChatId {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
    },
    /// Reset Telegram pairing state so the bot prompts for a new pairing code
    TelegramReset {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
    },
    /// Upload a SKILL.md to the skills API and deploy it to the OpenClaw instance
    SkillUpload {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Path to the local SKILL.md file
        #[arg(long)]
        file: PathBuf,
        /// Skills API base URL
        #[arg(long, env = "SKILLS_API_URL")]
        api_url: String,
        /// Skills API key
        #[arg(long, env = "USER_SKILLS_API_KEY")]
        api_key: String,
    },
    /// Download a SKILL.md from the skills API
    SkillDownload {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Output file path (default: ./SKILL.md)
        #[arg(long, default_value = "SKILL.md")]
        output: PathBuf,
        /// Skills API base URL
        #[arg(long, env = "SKILLS_API_URL")]
        api_url: String,
        /// Skills API key
        #[arg(long, env = "USER_SKILLS_API_KEY")]
        api_key: String,
    },
    /// Push a SKILL.md from the skills API to the OpenClaw instance
    SkillPush {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Skills API base URL
        #[arg(long, env = "SKILLS_API_URL")]
        api_url: String,
        /// Skills API key
        #[arg(long, env = "USER_SKILLS_API_KEY")]
        api_key: String,
    },
    /// Approve all pending webchat device pairing requests on a deployed instance
    DeviceApprove {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
    },
    /// Enable Tailscale Funnel on a deployed instance
    FunnelOn {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Port to expose via Funnel (default: 18789)
        #[arg(long, default_value = "18789")]
        port: u16,
    },
    /// Disable Tailscale Funnel on a deployed instance
    FunnelOff {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
    },
    /// Set up Tailscale Funnel on a deployed instance for public HTTPS access
    TailscaleFunnel {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Tailscale auth key (tskey-auth-...)
        #[arg(long, env = "TAILSCALE_AUTH_KEY")]
        auth_key: String,
        /// Port to expose via Funnel (default: 18789)
        #[arg(long, default_value = "18789")]
        port: u16,
    },
    /// Generate a temporary BytePlus ARK API key, or list available endpoints
    #[cfg(feature = "byteplus")]
    ArkApiKey {
        /// BytePlus Access Key
        #[arg(long, env = "BYTEPLUS_ACCESS_KEY")]
        access_key: String,
        /// BytePlus Secret Key
        #[arg(long, env = "BYTEPLUS_SECRET_KEY")]
        secret_key: String,
        /// List available ARK endpoints instead of generating a key
        #[arg(long)]
        list: bool,
        /// Resource type: "endpoint" or "bot"
        #[arg(long, default_value = "endpoint")]
        resource_type: String,
        /// Comma-separated endpoint or bot IDs the key is scoped to
        #[arg(long, value_delimiter = ',')]
        resource_ids: Vec<String>,
        /// Key validity duration in seconds (default: 7 days, max: 30 days)
        #[arg(long, default_value = "604800")]
        duration: u64,
    },
    /// Send a chat prompt to a BytePlus ARK model endpoint
    #[cfg(feature = "byteplus")]
    ArkChat {
        /// ARK API key (Bearer token)
        #[arg(long, env = "ARK_API_KEY")]
        api_key: String,
        /// Endpoint ID (e.g., ep-20260315233753-58rpv)
        #[arg(long, env = "ARK_ENDPOINT_ID")]
        endpoint_id: String,
        /// The prompt to send
        prompt: String,
    },
    /// Create a named snapshot from a DigitalOcean droplet
    #[cfg(feature = "digitalocean")]
    DoSnapshot {
        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: String,
        /// Droplet ID to snapshot
        #[arg(long)]
        droplet_id: u64,
        /// Name for the snapshot
        #[arg(long)]
        snapshot_name: String,
        /// Shut down the droplet before snapshotting, then power it back on
        #[arg(long)]
        power_off: bool,
    },
    /// Restore a DigitalOcean droplet from a snapshot
    #[cfg(feature = "digitalocean")]
    DoRestore {
        /// DigitalOcean API token
        #[arg(long, env = "DO_TOKEN")]
        do_token: String,
        /// Name of the snapshot to restore from
        #[arg(long)]
        snapshot_name: String,
        /// Region override (default: sgp1)
        #[arg(long)]
        region: Option<String>,
        /// Instance size override (default: s-2vcpu-4gb)
        #[arg(long)]
        size: Option<String>,
    },
    /// Create a snapshot of a Lightsail instance
    #[cfg(feature = "lightsail")]
    LsSnapshot {
        /// Instance name to snapshot
        #[arg(long)]
        instance_name: String,
        /// Name for the snapshot
        #[arg(long)]
        snapshot_name: String,
        /// AWS region (default: ap-southeast-1)
        #[arg(long, default_value = "ap-southeast-1")]
        region: String,
    },
    /// Restore a Lightsail instance from a snapshot
    #[cfg(feature = "lightsail")]
    LsRestore {
        /// Name of the snapshot to restore from
        #[arg(long)]
        snapshot_name: String,
        /// AWS region (default: ap-southeast-1)
        #[arg(long, default_value = "ap-southeast-1")]
        region: String,
        /// Instance size override
        #[arg(long)]
        size: Option<String>,
    },
    /// Create a snapshot of a BytePlus ECS instance's system disk
    #[cfg(feature = "byteplus")]
    BpSnapshot {
        /// BytePlus Access Key
        #[arg(long, env = "BYTEPLUS_ACCESS_KEY")]
        access_key: String,
        /// BytePlus Secret Key
        #[arg(long, env = "BYTEPLUS_SECRET_KEY")]
        secret_key: String,
        /// Instance ID to snapshot
        #[arg(long)]
        instance_id: String,
        /// Name for the snapshot
        #[arg(long)]
        snapshot_name: String,
        /// Region (default: ap-southeast-1)
        #[arg(long, default_value = "ap-southeast-1")]
        region: String,
    },
    /// Restore a BytePlus ECS instance from a snapshot
    #[cfg(feature = "byteplus")]
    BpRestore {
        /// BytePlus Access Key
        #[arg(long, env = "BYTEPLUS_ACCESS_KEY")]
        access_key: String,
        /// BytePlus Secret Key
        #[arg(long, env = "BYTEPLUS_SECRET_KEY")]
        secret_key: String,
        /// Name of the snapshot to restore from
        #[arg(long)]
        snapshot_name: String,
        /// Region (default: ap-southeast-1)
        #[arg(long, default_value = "ap-southeast-1")]
        region: String,
        /// Instance size override
        #[arg(long)]
        size: Option<String>,
        /// Use spot instance
        #[arg(long)]
        spot: bool,
    },
    /// Update the AI model on a deployed OpenClaw instance
    UpdateModel {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Primary AI model (anthropic, openai, gemini, byteplus)
        #[arg(long)]
        primary_model: String,
        /// First failover model
        #[arg(long, default_value = "")]
        failover_1: String,
        /// Second failover model
        #[arg(long, default_value = "")]
        failover_2: String,
        /// Anthropic API key (only needed if changing to/adding anthropic)
        #[arg(long, default_value = "", env = "ANTHROPIC_API_KEY")]
        anthropic_key: String,
        /// OpenAI API key (only needed if changing to/adding openai)
        #[arg(long, default_value = "", env = "OPENAI_API_KEY")]
        openai_key: String,
        /// Gemini API key (only needed if changing to/adding gemini)
        #[arg(long, default_value = "", env = "GEMINI_API_KEY")]
        gemini_key: String,
        /// BytePlus Ark API key (only needed if changing to/adding byteplus)
        #[arg(long, default_value = "", env = "BYTEPLUS_ARK_API_KEY")]
        byteplus_ark_api_key: String,
    },
    /// Set up WhatsApp on a deployed instance (set phone number, enable plugin, fetch QR)
    WhatsappSetup {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// WhatsApp phone number (with country code, e.g. +6512345678)
        #[arg(long)]
        phone_number: String,
        /// Clear existing session credentials before setup (reset + setup in one SSH session)
        #[arg(long)]
        reset: bool,
    },
    /// Fetch the WhatsApp pairing QR code from a deployed instance
    WhatsappQr {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
    },
    /// Reset WhatsApp pairing state so a new QR code scan is required
    WhatsappReset {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
    },
    /// List available OpenClaw versions from the npm registry
    OpenclawVersions {
        /// Output as JSON array
        #[arg(long)]
        json: bool,
    },
    /// Install a specific OpenClaw version on a deployed instance
    OpenclawInstall {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Version to install (e.g. 2026.3.20)
        #[arg(long)]
        version: String,
    },
    /// Deploy a ZIP of OpenClaw skills to an instance workspace and restart the gateway
    SkillDeploy {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Path to the .zip file containing skills
        #[arg(long)]
        file: std::path::PathBuf,
    },
    /// Check drift between a local skill directory and the deployed skill on an instance
    SkillDiff {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Path to the local skill directory (e.g. ./newsclaw-skills)
        #[arg(long)]
        dir: std::path::PathBuf,
    },
    /// Remove a deployed skill from the instance workspace and restart the gateway
    SkillRemove {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Name of the skill directory to remove (e.g. newsclaw-skills)
        #[arg(long)]
        skill: String,
    },
    /// List all skill directories deployed on an instance
    SkillList {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
    },
    /// Check (and optionally fix) file ownership and permissions for a deployed skill
    SkillCheckPerms {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Name of the skill to check (e.g. newsclaw-skills)
        #[arg(long)]
        skill: String,
        /// Automatically fix any ownership/permission issues found
        #[arg(long)]
        fix: bool,
    },
    /// Install an OpenClaw plugin on a deployed instance and restart the gateway
    PluginInstall {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Plugin package name (e.g. @openguardrails/moltguard)
        #[arg(long)]
        plugin: String,
    },
    /// Add a scheduled message cron job on an OpenClaw instance
    ///
    /// The gateway agent receives the message on schedule and delivers the
    /// response to the chosen channel (telegram, whatsapp, etc.).
    CronMessage {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Unique job name
        #[arg(long)]
        name: String,
        /// Cron expression (5-field, e.g. "0 9 * * *" for 9 AM daily)
        #[arg(long)]
        schedule: Option<String>,
        /// Run every duration instead of a cron expression (e.g. 30m, 1h, 6h)
        #[arg(long)]
        every: Option<String>,
        /// Message to send to the agent (e.g. "What's happening today?")
        #[arg(long)]
        message: String,
        /// Delivery channel for the response (last, telegram, whatsapp, discord …)
        /// Use "last" to deliver to whoever last messaged the bot (recommended).
        #[arg(long, default_value = "last")]
        channel: String,
        /// Specific recipient (E.164 phone number or Telegram chat ID)
        #[arg(long)]
        to: Option<String>,
    },
    /// Add a scheduled tool-execution cron job on an OpenClaw instance
    ///
    /// Asks the gateway agent to run a specific installed tool on schedule and
    /// deliver the result to the chosen channel.
    CronTool {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Unique job name
        #[arg(long)]
        name: String,
        /// Cron expression (5-field, e.g. "0 * * * *" for every hour)
        #[arg(long)]
        schedule: Option<String>,
        /// Run every duration instead of a cron expression (e.g. 30m, 1h, 6h)
        #[arg(long)]
        every: Option<String>,
        /// Tool name to execute (as installed on the instance)
        #[arg(long)]
        tool: String,
        /// Arguments / context to pass to the tool
        #[arg(long, default_value = "")]
        args: String,
        /// Delivery channel for the result (last, telegram, whatsapp, discord …)
        /// Use "last" to deliver to whoever last messaged the bot (recommended).
        #[arg(long, default_value = "last")]
        channel: String,
        /// Specific recipient (E.164 phone number or Telegram chat ID)
        #[arg(long)]
        to: Option<String>,
    },
    /// List all cron jobs on an OpenClaw instance
    CronList {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
    },
    /// Remove a cron job by name from an OpenClaw instance
    CronRemove {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
        /// Job name to remove
        #[arg(long)]
        name: String,
    },
    /// Refresh the IP address of a deployed instance from the cloud provider
    UpdateIp {
        /// Deploy ID, hostname, or IP address of the instance
        #[arg(long)]
        instance: String,
    },
    /// Start the web UI server
    #[cfg(feature = "web-ui")]
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "3456")]
        port: u16,
    },
}

fn main() -> anyhow::Result<()> {
    // Windows default stack (1 MB) is too small for complex async futures and Axum's
    // large router type.  Spawn on a thread with 8 MB to match Linux behaviour.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime")
                .block_on(async_main())
        })?
        .join()
        .map_err(|_| anyhow::anyhow!("Main thread panicked"))?
}

async fn async_main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Deploy {
            provider,
            customer_name,
            customer_email,
            do_token,
            tencent_secret_id,
            tencent_secret_key,
            aws_access_key_id,
            aws_secret_access_key,
            aws_region,
            azure_tenant_id,
            azure_subscription_id,
            azure_client_id,
            azure_client_secret,
            byteplus_access_key,
            byteplus_secret_key,
            byteplus_ark_api_key,
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
            primary_model,
            failover_1,
            failover_2,
            profile,
            spot,
            openclaw_version,
            detach,
            json,
            _deploy_id,
        } => {
            commands::deploy_cmd::run(commands::deploy_cmd::DeployCmdArgs {
                provider,
                customer_name,
                customer_email,
                do_token,
                tencent_secret_id,
                tencent_secret_key,
                aws_access_key_id,
                aws_secret_access_key,
                aws_region,
                azure_tenant_id,
                azure_subscription_id,
                azure_client_id,
                azure_client_secret,
                byteplus_access_key,
                byteplus_secret_key,
                byteplus_ark_api_key,
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
                primary_model,
                failover_1,
                failover_2,
                profile,
                spot,
                openclaw_version,
                detach,
                json,
                deploy_id: _deploy_id,
            })
            .await
        }
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
            aws_access_key_id,
            aws_secret_access_key,
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
                aws_access_key_id,
                aws_secret_access_key,
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
        Commands::TelegramSetup {
            instance,
            bot_token,
            reset,
        } => commands::telegram::configure_bot(&instance, &bot_token, reset).await,
        Commands::TelegramPair { instance, code } => {
            commands::telegram::approve_pairing(&instance, &code).await
        }
        Commands::TelegramChatId { instance } => commands::telegram::get_chat_id(&instance).await,
        Commands::TelegramReset { instance } => commands::telegram::reset(&instance).await,
        Commands::SkillDeploy { instance, file } => {
            commands::skill_deploy::deploy(&instance, &file).await
        }
        Commands::SkillDiff { instance, dir } => commands::skill_diff::diff(&instance, &dir).await,
        Commands::SkillRemove { instance, skill } => {
            commands::skill_remove::remove(&instance, &skill).await
        }
        Commands::SkillList { instance } => commands::skill_remove::list(&instance).await,
        Commands::SkillCheckPerms {
            instance,
            skill,
            fix,
        } => commands::skill_remove::check_permissions(&instance, &skill, fix).await,
        Commands::SkillUpload {
            instance,
            file,
            api_url,
            api_key,
        } => commands::skill::upload(&instance, &file, &api_url, &api_key).await,
        Commands::SkillDownload {
            instance,
            output,
            api_url,
            api_key,
        } => commands::skill::download(&instance, &output, &api_url, &api_key).await,
        Commands::SkillPush {
            instance,
            api_url,
            api_key,
        } => commands::skill::push_to_instance(&instance, &api_url, &api_key).await,
        Commands::DeviceApprove { instance } => {
            commands::tailscale_funnel::device_approve_all(&instance).await
        }
        Commands::FunnelOn { instance, port } => {
            commands::tailscale_funnel::funnel_on(&instance, port).await
        }
        Commands::FunnelOff { instance } => commands::tailscale_funnel::funnel_off(&instance).await,
        Commands::TailscaleFunnel {
            instance,
            auth_key,
            port,
        } => commands::tailscale_funnel::setup(&instance, &auth_key, port).await,
        #[cfg(feature = "byteplus")]
        Commands::ArkApiKey {
            access_key,
            secret_key,
            list,
            resource_type,
            resource_ids,
            duration,
        } => {
            if list {
                commands::ark::list_endpoints(&access_key, &secret_key).await
            } else {
                commands::ark::get_api_key(
                    &access_key,
                    &secret_key,
                    &resource_type,
                    &resource_ids,
                    duration,
                )
                .await
            }
        }
        #[cfg(feature = "byteplus")]
        Commands::ArkChat {
            api_key,
            endpoint_id,
            prompt,
        } => commands::ark::chat(&api_key, &endpoint_id, &prompt).await,
        #[cfg(feature = "digitalocean")]
        Commands::DoSnapshot {
            do_token,
            droplet_id,
            snapshot_name,
            power_off,
        } => {
            commands::do_snapshot::run(commands::do_snapshot::DoSnapshotParams {
                do_token,
                droplet_id,
                snapshot_name,
                power_off,
                progress_tx: None,
                db: None,
                op_id: None,
            })
            .await
        }
        #[cfg(feature = "digitalocean")]
        Commands::DoRestore {
            do_token,
            snapshot_name,
            region,
            size,
        } => commands::do_restore::run(commands::do_restore::DoRestoreParams {
            do_token,
            snapshot_name,
            region,
            size,
            progress_tx: None,
            db: None,
            op_id: None,
        })
        .await
        .map(|_| ()),
        #[cfg(feature = "lightsail")]
        Commands::LsSnapshot {
            instance_name,
            snapshot_name,
            region,
        } => {
            commands::ls_snapshot::run(commands::ls_snapshot::LsSnapshotParams {
                instance_name,
                snapshot_name,
                region,
                progress_tx: None,
                db: None,
                op_id: None,
            })
            .await
        }
        #[cfg(feature = "lightsail")]
        Commands::LsRestore {
            snapshot_name,
            region,
            size,
        } => commands::ls_restore::run(commands::ls_restore::LsRestoreParams {
            snapshot_name,
            region,
            size,
            progress_tx: None,
            db: None,
            op_id: None,
        })
        .await
        .map(|_| ()),
        #[cfg(feature = "byteplus")]
        Commands::BpSnapshot {
            access_key,
            secret_key,
            instance_id,
            snapshot_name,
            region,
        } => {
            commands::bp_snapshot::run(commands::bp_snapshot::BpSnapshotParams {
                access_key,
                secret_key,
                instance_id,
                snapshot_name,
                region,
                progress_tx: None,
                db: None,
                op_id: None,
            })
            .await
        }
        #[cfg(feature = "byteplus")]
        Commands::BpRestore {
            access_key,
            secret_key,
            snapshot_name,
            region,
            size,
            spot,
        } => commands::bp_restore::run(commands::bp_restore::BpRestoreParams {
            access_key,
            secret_key,
            snapshot_name,
            region,
            size,
            spot,
            progress_tx: None,
            db: None,
            op_id: None,
        })
        .await
        .map(|_| ()),
        Commands::UpdateModel {
            instance,
            primary_model,
            failover_1,
            failover_2,
            anthropic_key,
            openai_key,
            gemini_key,
            byteplus_ark_api_key,
        } => {
            commands::update_model::run(commands::update_model::UpdateModelParams {
                instance,
                primary_model,
                failover_1,
                failover_2,
                anthropic_key,
                openai_key,
                gemini_key,
                byteplus_ark_api_key,
            })
            .await
        }
        Commands::WhatsappSetup {
            instance,
            phone_number,
            reset,
        } => commands::whatsapp_setup::setup(&instance, &phone_number, reset).await,
        Commands::WhatsappQr { instance } => commands::whatsapp_setup::fetch_qr(&instance).await,
        Commands::WhatsappReset { instance } => commands::whatsapp_setup::reset(&instance).await,
        Commands::OpenclawVersions { json } => commands::openclaw_version::run_list(json).await,
        Commands::OpenclawInstall { instance, version } => {
            commands::openclaw_version::run_install(&instance, &version).await
        }
        Commands::PluginInstall { instance, plugin } => {
            commands::plugin_install::run(&instance, &plugin).await
        }
        Commands::CronMessage {
            instance,
            name,
            schedule,
            every,
            message,
            channel,
            to,
        } => {
            commands::cron_schedule::add_message(
                &instance, &name, &schedule, &every, &message, &channel, &to,
            )
            .await
        }
        Commands::CronTool {
            instance,
            name,
            schedule,
            every,
            tool,
            args,
            channel,
            to,
        } => {
            commands::cron_schedule::add_tool(commands::cron_schedule::AddToolParams {
                query: &instance,
                name: &name,
                schedule: &schedule,
                every: &every,
                tool: &tool,
                args: &args,
                channel: &channel,
                to: &to,
            })
            .await
        }
        Commands::CronList { instance } => commands::cron_schedule::list(&instance).await,
        Commands::CronRemove { instance, name } => {
            commands::cron_schedule::remove(&instance, &name).await
        }
        Commands::UpdateIp { instance } => commands::update_ip::run(&instance).await,
        #[cfg(feature = "web-ui")]
        Commands::Serve { port } => commands::serve::run(port).await,
    }
}
