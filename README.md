# clawmacdo

[![Release](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml)
[![Changelog](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml)

Rust CLI tool for deploying [OpenClaw](https://openclaw.ai) to **DigitalOcean**, **AWS Lightsail**, **Tencent Cloud**, **Microsoft Azure**, or **BytePlus Cloud** — with Claude Code, Codex, and Gemini CLI pre-installed.

## ✨ What's New in v0.20.0

- **`do-restore` subcommand** — restore a DigitalOcean droplet from a snapshot by name, with standard `openclaw-{id}` naming and deploy record saved to both JSON and SQLite (visible in web UI Deployments tab)

### Previous highlights (v0.19.x)
- **One-click Funnel access** — "Open" button in Deployments tab opens the Funnel webchat with gateway token pre-injected (no manual token paste or device pairing needed)
- **Auto-disable device pairing for Funnel** — Funnel setup sets `dangerouslyDisableDeviceAuth: true` so browser connections via Tailscale Funnel skip the pairing screen

### Previous highlights (v0.18.x)
- **Tailscale Funnel** — `tailscale-funnel` subcommand: install Tailscale, enable Funnel, configure `openclaw.json`, auto-approve devices, and print public webchat URL
- **Funnel toggle** — `funnel-on` / `funnel-off` CLI commands and web UI Deployments tab toggle button
- **Customer skill management** — `skill-upload`, `skill-download`, `skill-push` subcommands for per-deployment SKILL.md

### Previous highlights (v0.17.x)
- **Web UI security hardening (CRIT-01)** — API key auth, 6-digit PIN login, CORS, rate limiting, localhost-only binding
- **All 4 CRITICAL security findings resolved**

### Previous highlights (v0.16.x)
- **BytePlus destroy cleanup** — Auto-release EIP and delete VPC/subnet/security-group
- **Playwright E2E test suite** — 30 CSV-driven test scenarios covering all 5 cloud providers

### Previous highlights (v0.14.x – v0.15.x)
- **Windows builds fixed** — Dependencies correctly scoped, native MSVC builds
- **`digitalocean` feature flag** — DigitalOcean provider now properly gated as a default feature

### Previous highlights (v0.13.x)
- **`ark-api-key`** — Generate temporary BytePlus ARK API keys from access/secret credentials, or list endpoints with `--list`
- **`ark-chat`** — Send prompts to BytePlus ARK models directly from the CLI
- **`telegram-setup` / `telegram-pair`** — Configure and pair Telegram bots on deployed instances via SSH
- **Web UI destroy** — Destroy cloud instances directly from the Deployments tab with provider-specific credential prompts
- **Detach mode improvements** — Proper `setsid()` session detachment, stdout/stderr logging to file
- **Workspace path fix** — Automatic `/root/` → `/home/openclaw/` path correction during provisioning

### Earlier highlights (v0.9.x – v0.12.x)
- **BytePlus Cloud** — 5th cloud provider added (`--provider=byteplus` or `bp`)
- **BytePlus ECS client** — HMAC-SHA256 signed REST API with auto-provisioning of VPC, subnet, and security group
- **Preflight CLI checks** — Azure CLI and AWS CLI verified at startup, auto-installed if missing
- **Full-width professional web UI** — layout widened to 1536px max, compact hero with inline mascot
- **Deploy progress tracking** — All 16 deploy steps persisted to SQLite in real-time
- **`clawmacdo track` command** — Query deploy progress by ID, hostname, or IP address
- **Follow mode** (`--follow`) — Live-polling display that refreshes until deployment finishes
- **5 cloud providers** — DigitalOcean, AWS Lightsail, Tencent Cloud, Microsoft Azure, BytePlus Cloud
- **npm distribution** — `npm install -g clawmacdo`

## 🏗️ Project Structure

```
clawmacdo/
├── Cargo.toml              # Workspace configuration
├── crates/                 # All crates in workspace
│   ├── clawmacdo-cli/      # 🖥️  Main CLI binary & command orchestration
│   ├── clawmacdo-core/     # 🔧  Config, errors, shared types
│   ├── clawmacdo-cloud/    # ☁️   Cloud provider implementations
│   ├── clawmacdo-provision/# 🔨  Server provisioning & setup logic  
│   ├── clawmacdo-db/       # 💾  Database operations & storage
│   ├── clawmacdo-ssh/      # 🔑  SSH/SCP operations & key management
│   └── clawmacdo-ui/       # 🎨  Web UI, progress bars, user prompts
├── assets/                 # Static assets (mascot, etc.)
└── README.md
```

### 📦 Crate Overview

| Crate | Purpose | Dependencies |
|-------|---------|--------------|
| **clawmacdo-cli** | Main binary, command parsing, orchestration | All other crates |
| **clawmacdo-core** | Configuration, errors, shared types | Minimal (serde, anyhow) |
| **clawmacdo-cloud** | DigitalOcean, AWS Lightsail, Tencent Cloud & BytePlus APIs | reqwest, async-trait |
| **clawmacdo-provision** | Server setup, package installation | SSH, Core, UI |
| **clawmacdo-db** | SQLite operations, job tracking | rusqlite |
| **clawmacdo-ssh** | SSH connections, file transfers | ssh2 |
| **clawmacdo-ui** | Progress bars, web interface | indicatif, axum |

## Features

- **Multi-cloud**: Deploy to DigitalOcean, AWS Lightsail, Tencent Cloud, Microsoft Azure, or BytePlus Cloud with `--provider` flag
- **Backup** local `~/.openclaw/` config into a timestamped `.tar.gz`
- **1-click deploy**: generate SSH keys, provision a cloud instance, install Node 24 + OpenClaw + Claude Code + Codex + Gemini CLI, restore config, configure `.env` (API + messaging), start the gateway, and auto-configure model failover
- **Cloud-to-cloud migration**: SSH into a source instance, back up remotely, deploy to a new instance, restore
- **Snapshot restore**: create a DigitalOcean droplet from a snapshot by name, with deploy record saved to SQLite for web UI visibility
- **Destroy**: delete an instance by name with confirmation, clean up SSH keys (cloud + local)
- **Status**: list all openclaw-tagged instances with IPs
- **List backups**: show local backup archives with sizes and dates
- **Web UI**: Browser-based deploy interface with real-time SSE progress streaming (optional)
- **Security groups**: Auto-create firewall rules on Tencent Cloud and BytePlus (SSH + HTTP/HTTPS + Gateway)

## Supported Cloud Providers

| Provider | Flag | Credentials | Prerequisite |
|----------|------|-------------|-------------|
| DigitalOcean | `--provider=digitalocean` (default) | `--do-token` | — |
| AWS Lightsail | `--provider=lightsail` (or `aws`) | `--aws-access-key-id` + `--aws-secret-access-key` | [AWS CLI](https://aws.amazon.com/cli/) installed |
| Tencent Cloud | `--provider=tencent` | `--tencent-secret-id` + `--tencent-secret-key` | — |
| Microsoft Azure | `--provider=azure` (or `az`) | `--azure-tenant-id` + `--azure-subscription-id` + `--azure-client-id` + `--azure-client-secret` | [Azure CLI](https://learn.microsoft.com/en-us/cli/azure/) installed |
| BytePlus Cloud | `--provider=byteplus` (or `bp`) | `--byteplus-access-key` + `--byteplus-secret-key` | — |

## Download

Pre-built binaries for every release are available on the [Releases page](https://github.com/kenken64/clawmacdo/releases):

| Platform | Architecture | Full Build | Minimal Build |
|----------|-------------|------------|---------------|
| Linux    | x86_64      | `clawmacdo-linux-amd64-full.tar.gz` | `clawmacdo-linux-amd64-minimal.tar.gz` |
| macOS    | Apple Silicon (arm64) | `clawmacdo-darwin-arm64-full.tar.gz` | `clawmacdo-darwin-arm64-minimal.tar.gz` |
| Windows  | x86_64      | `clawmacdo-windows-amd64-full.zip` | `clawmacdo-windows-amd64-minimal.zip` |

## Installation

### From npm (recommended)

```bash
npm install -g clawmacdo
```

### From release binary

Download the archive for your platform from [Releases](https://github.com/kenken64/clawmacdo/releases), extract, and add to your `PATH`.

### From source

#### Full build (all features)
```bash
cargo build --release
# Binary: target/release/clawmacdo (4.6MB)
```

#### Minimal build (CLI only, no web UI)
```bash
cargo build --release --no-default-features --features minimal
# Binary: target/release/clawmacdo (3.1MB - 32% smaller!)
```

#### DigitalOcean-only build
```bash
cargo build --release --no-default-features --features digitalocean-only
# Binary: target/release/clawmacdo (3.1MB, no Tencent Cloud)
```

#### AWS Lightsail-only build
```bash
cargo build --release --no-default-features --features aws-only
# Binary: target/release/clawmacdo (Lightsail only, requires AWS CLI)
```

## Build Features

| Feature | Description | Default |
|---------|-------------|---------|
| `web-ui` | Browser-based deployment interface | ✅ |
| `lightsail` | AWS Lightsail provider support (via AWS CLI) | ✅ |
| `tencent-cloud` | Tencent Cloud provider support | ✅ |
| `azure` | Microsoft Azure provider support (via Azure CLI) | ✅ |
| `byteplus` | BytePlus Cloud provider support | ✅ |
| `digitalocean` | DigitalOcean provider support | ✅ |
| `aws-only` | Lightsail-only build (no DO or Tencent) | ❌ |
| `minimal` | CLI-only, no web UI or optional features | ❌ |

## Usage

> **Full CLI reference with all examples, curl commands, and sample responses:** [docs/clawmacdo_usage.md](docs/clawmacdo_usage.md)

### Deploy OpenClaw to DigitalOcean

```bash
# Set your DO token
export DO_TOKEN="your_digitalocean_api_token"

# Deploy with backup & restore
clawmacdo deploy \
  --customer-name "my-openclaw" \
  --restore-from ~/backups/openclaw-backup-2024-03-09.tar.gz
```

### Deploy to AWS Lightsail

> **Prerequisite:** [AWS CLI](https://aws.amazon.com/cli/) must be installed and accessible in your `PATH`.

```bash
# Set AWS credentials
export AWS_ACCESS_KEY_ID="your_access_key_id"
export AWS_SECRET_ACCESS_KEY="your_secret_access_key"
export AWS_REGION="us-east-1"  # default region

# Deploy to Lightsail
clawmacdo deploy \
  --provider lightsail \
  --customer-name "my-openclaw" \
  --customer-email "you@example.com" \
  --aws-region us-east-1
```

#### Lightsail Instance Sizes

| clawmacdo `--size` | Lightsail Bundle | vCPU | RAM | Price |
|--------------------|-----------------|------|-----|-------|
| `s-1vcpu-2gb` | `small_3_0` | 1 | 2 GB | ~$10/mo |
| `s-2vcpu-4gb` *(default)* | `medium_3_0` | 2 | 4 GB | ~$20/mo |
| `s-4vcpu-8gb` | `large_3_0` | 4 | 8 GB | ~$40/mo |

### Deploy to Tencent Cloud

```bash
# Set Tencent credentials
export TENCENT_SECRET_ID="your_secret_id"
export TENCENT_SECRET_KEY="your_secret_key"

# Deploy to Hong Kong region
clawmacdo deploy \
  --provider tencent \
  --customer-name "my-openclaw-hk" \
  --region ap-hongkong
```

### Deploy to BytePlus Cloud

```bash
# Set BytePlus credentials
export BYTEPLUS_ACCESS_KEY="your_access_key"
export BYTEPLUS_SECRET_KEY="your_secret_key"

# Deploy with BytePlus ARK as primary AI model
clawmacdo deploy \
  --provider byteplus \
  --customer-name "my-openclaw-bp" \
  --region ap-southeast-1 \
  --primary-model byteplus \
  --byteplus-ark-api-key "$BYTEPLUS_ARK_API_KEY" \
  --anthropic-key "$ANTHROPIC_API_KEY"
```

#### BytePlus Instance Sizes

| clawmacdo `--size` | vCPU | RAM | Notes |
|--------------------|------|-----|-------|
| `ecs.c3i.large` | 2 | 4 GB | Compute-optimized |
| `ecs.g3i.large` *(default)* | 2 | 8 GB | General purpose |
| `ecs.c3i.xlarge` | 4 | 8 GB | Compute-optimized |
| `ecs.g3i.xlarge` | 4 | 16 GB | General purpose |

### AI Model Configuration

Set a primary AI model and optional failovers for the deployed instance. Supported models: `anthropic`, `openai`, `gemini`, `byteplus`.

```bash
# Anthropic as primary (default)
clawmacdo deploy --provider do --customer-email "user@example.com" \
  --primary-model anthropic --anthropic-key "$ANTHROPIC_API_KEY"

# BytePlus ARK as primary with Anthropic failover
clawmacdo deploy --provider bp --customer-email "user@example.com" \
  --primary-model byteplus --failover-1 anthropic \
  --byteplus-ark-api-key "$BYTEPLUS_ARK_API_KEY" \
  --anthropic-key "$ANTHROPIC_API_KEY"

# Multi-model failover chain
clawmacdo deploy --provider do --customer-email "user@example.com" \
  --primary-model anthropic --failover-1 openai --failover-2 gemini \
  --anthropic-key "$ANTHROPIC_API_KEY" \
  --openai-key "$OPENAI_API_KEY" --gemini-key "$GEMINI_API_KEY"
```

| Model | `--primary-model` value | Model identifier | Required flag |
|-------|------------------------|------------------|---------------|
| Anthropic Claude | `anthropic` | `anthropic/claude-opus-4-6` | `--anthropic-key` |
| OpenAI | `openai` | `openai/gpt-5-mini` | `--openai-key` |
| Google Gemini | `gemini` | `google/gemini-2.5-flash` | `--gemini-key` |
| BytePlus ARK | `byteplus` | `byteplus/ark-code-latest` | `--byteplus-ark-api-key` |

### ARK API Key Management

Generate temporary BytePlus ARK API keys or list available endpoints.

```bash
# List available ARK endpoints
clawmacdo ark-api-key --list

# Generate a 7-day API key for an endpoint
clawmacdo ark-api-key \
  --resource-ids ep-20260315233753-58rpv

# Generate a 30-day key for multiple endpoints
clawmacdo ark-api-key \
  --resource-ids ep-abc123,ep-def456 \
  --duration 2592000
```

### ARK Chat

Send chat prompts directly to BytePlus ARK model endpoints from the CLI.

```bash
# Direct usage
clawmacdo ark-chat \
  --api-key "$ARK_API_KEY" \
  --endpoint-id ep-20260315233753-58rpv \
  "Hello, what model are you?"

# Using environment variables
export ARK_API_KEY="your_ark_api_key"
export ARK_ENDPOINT_ID="ep-20260315233753-58rpv"
clawmacdo ark-chat "Explain quantum computing in 3 sentences."
```

### Restore DigitalOcean Droplet from Snapshot

Create a new droplet from an existing DigitalOcean snapshot. The droplet name follows the standard `openclaw-{id}` naming convention.

```bash
# Restore from a snapshot by name
clawmacdo do-restore \
  --do-token "$DO_TOKEN" \
  --snapshot-name "my-openclaw-snapshot"

# With region and size overrides
clawmacdo do-restore \
  --do-token "$DO_TOKEN" \
  --snapshot-name "my-openclaw-snapshot" \
  --region nyc1 \
  --size s-4vcpu-8gb
```

The command generates a new SSH key pair, looks up the snapshot by name, creates the droplet, waits for it to become active, and saves a deploy record for use with other `clawmacdo` commands.

### Track Deploy Progress

```bash
# Track by deploy ID, hostname, or IP
clawmacdo track <deploy-id>

# Follow mode — live refresh until complete
clawmacdo track <deploy-id> --follow

# JSON output (NDJSON)
clawmacdo track <deploy-id> --json
```

### Web UI Mode

```bash
# Start browser interface
clawmacdo serve --port 3456
# Open http://localhost:3456
```

### Cloud Migration

```bash
# Migrate from one cloud to another
clawmacdo migrate \
  --source-ip 1.2.3.4 \
  --source-ssh-key ~/.ssh/old_instance \
  --target-provider tencent \
  --customer-name "migrated-openclaw"
```

### Backup & Restore

```bash
# Create local backup
clawmacdo backup

# List backups
clawmacdo list-backups

# Deploy with specific backup
clawmacdo deploy --restore-from ~/.openclaw/backups/openclaw-2024-03-09_14-30-15.tar.gz
```

## Examples

### Full Deploy with All Options

```bash
clawmacdo deploy \
  --provider digitalocean \
  --customer-name "production-openclaw" \
  --customer-email "admin@company.com" \
  --size s-2vcpu-4gb \
  --region nyc1 \
  --primary-model anthropic \
  --failover-1 openai \
  --failover-2 gemini \
  --anthropic-key "$ANTHROPIC_API_KEY" \
  --openai-key "$OPENAI_API_KEY" \
  --gemini-key "$GEMINI_API_KEY" \
  --telegram-bot-token "$TELEGRAM_TOKEN" \
  --whatsapp-phone-number "+1234567890" \
  --tailscale \
  --tailscale-auth-key "$TAILSCALE_AUTH" \
  --backup ~/openclaw-backup.tar.gz
```

### Quick Status Check

```bash
# List all instances
clawmacdo status

# Check specific provider
clawmacdo status --provider tencent
```

## Development

### Workspace Commands

```bash
# Build all crates
cargo build

# Test all crates
cargo test

# Build specific crate
cargo build -p clawmacdo-core

# Run clippy on workspace
cargo clippy --all

# Update dependencies
cargo update
```

### Adding Dependencies

Add to workspace `Cargo.toml`:
```toml
[workspace.dependencies]
new-crate = "1.0"
```

Then reference in individual crate:
```toml
[dependencies]
new-crate = { workspace = true }
```

## Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `DO_TOKEN` | DigitalOcean API token | For DO deploys |
| `AWS_ACCESS_KEY_ID` | AWS IAM access key ID | For Lightsail deploys |
| `AWS_SECRET_ACCESS_KEY` | AWS IAM secret access key | For Lightsail deploys |
| `AWS_REGION` | AWS region (default: `us-east-1`) | For Lightsail deploys |
| `TENCENT_SECRET_ID` | Tencent Cloud Secret ID | For Tencent deploys |
| `TENCENT_SECRET_KEY` | Tencent Cloud Secret Key | For Tencent deploys |
| `AZURE_TENANT_ID` | Azure AD tenant ID | For Azure deploys |
| `AZURE_SUBSCRIPTION_ID` | Azure subscription ID | For Azure deploys |
| `AZURE_CLIENT_ID` | Azure service principal client ID | For Azure deploys |
| `AZURE_CLIENT_SECRET` | Azure service principal client secret | For Azure deploys |
| `BYTEPLUS_ACCESS_KEY` | BytePlus Access Key | For BytePlus deploys |
| `BYTEPLUS_SECRET_KEY` | BytePlus Secret Key | For BytePlus deploys |
| `BYTEPLUS_ARK_API_KEY` | BytePlus ARK API key (for AI model inference) | For BytePlus ARK model |
| `ARK_API_KEY` | ARK bearer token for `ark-chat` | For `ark-chat` |
| `ARK_ENDPOINT_ID` | ARK endpoint ID for `ark-chat` | For `ark-chat` |
| `CLAUDE_API_KEY` | Anthropic Claude API key | Optional |
| `OPENAI_API_KEY` | OpenAI API key | Optional |
| `TELEGRAM_TOKEN` | Telegram bot token | Optional |
| `TAILSCALE_AUTH_KEY` | Tailscale auth key | Optional |
| `CLAWMACDO_API_KEY` | API key protecting `/api/*` endpoints | Optional (Web UI) |
| `CLAWMACDO_PIN` | 6-digit PIN for web UI login page | Optional (Web UI) |
| `CLAWMACDO_BIND` | Server bind address (default: `127.0.0.1`) | Optional (Web UI) |
| `SKILLS_API_URL` | Railway skills API base URL | For skill commands |
| `USER_SKILLS_API_KEY` | API key for user-skills endpoints | For skill commands |

## Architecture Notes

The refactored workspace follows a **dependency hierarchy**:

1. **clawmacdo-core** - Foundation (no internal deps)
2. **clawmacdo-ssh** - Depends on core
3. **clawmacdo-db** - Depends on core  
4. **clawmacdo-ui** - Depends on core
5. **clawmacdo-cloud** - Depends on core
6. **clawmacdo-provision** - Depends on core, ssh, ui, cloud
7. **clawmacdo-cli** - Orchestration layer (depends on all)

This prevents circular dependencies and enables clean testing.

## Performance Optimizations

- **LTO enabled** for release builds
- **Panic = abort** for smaller binaries
- **Symbol stripping** in release mode
- **Feature gates** for optional components
- **Minimal Tokio features** (not "full")

## Contributing

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality  
4. Run `cargo clippy` and `cargo test`
5. Submit a pull request

## License

MIT License - see [LICENSE](LICENSE) for details.

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for version history and breaking changes.

---

**Last updated:** March 17, 2026
**Current version:** 0.20.0
**Architecture version:** 2.0 (modular workspace)