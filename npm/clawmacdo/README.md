# clawmacdo

[![Release](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml)
[![Changelog](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml)

Rust CLI tool for deploying [OpenClaw](https://openclaw.ai) to **DigitalOcean**, **AWS Lightsail**, **Tencent Cloud**, **Microsoft Azure**, or **BytePlus Cloud** — with Claude Code, Codex, and Gemini CLI pre-installed.

## Features

- **Multi-cloud**: Deploy to DigitalOcean, AWS Lightsail, Tencent Cloud, Microsoft Azure, or BytePlus Cloud with `--provider` flag
- **1-click deploy**: generate SSH keys, provision a cloud instance, install Node 24 + OpenClaw + Claude Code + Codex + Gemini CLI, restore config, configure `.env` (API + messaging), start the gateway, and auto-configure model failover
- **Cloud-to-cloud migration**: SSH into a source instance, back up remotely, deploy to a new instance, restore
- **Snapshot & restore**: create and restore named snapshots for DigitalOcean, BytePlus, and AWS Lightsail
- **Destroy**: delete an instance by name with confirmation, clean up SSH keys (cloud + local)
- **Status**: list all openclaw-tagged instances with IPs
- **Backup**: back up local `~/.openclaw/` config into a timestamped `.tar.gz`
- **Web UI**: browser-based deploy interface with real-time SSE progress streaming (optional)
- **Security groups**: auto-create firewall rules on Tencent Cloud and BytePlus (SSH + HTTP/HTTPS + Gateway)

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

## Quick Start (CLI)

```bash
# Install
npm install -g clawmacdo

# Deploy to DigitalOcean
clawmacdo deploy --provider digitalocean \
  --customer-name "my-openclaw" --customer-email "you@example.com" \
  --do-token "$DO_TOKEN" --anthropic-key "$ANTHROPIC_API_KEY"

# Deploy to AWS Lightsail
clawmacdo deploy --provider lightsail \
  --customer-name "my-openclaw" --customer-email "you@example.com" \
  --aws-access-key-id "$AWS_ACCESS_KEY_ID" \
  --aws-secret-access-key "$AWS_SECRET_ACCESS_KEY"

# Track deploy progress
clawmacdo track <deploy-id> --follow

# Set up Telegram bot
clawmacdo telegram-setup --instance <deploy-id> --bot-token "$TELEGRAM_TOKEN"
clawmacdo telegram-setup --instance <deploy-id> --bot-token "$TELEGRAM_TOKEN" --reset  # reset + setup in one SSH session
clawmacdo telegram-pair --instance <deploy-id> --code <PAIRING_CODE>
clawmacdo telegram-chat-id --instance <deploy-id>
clawmacdo telegram-reset --instance <deploy-id>    # clear pairing, force new code

# Set up WhatsApp (displays QR code to scan)
clawmacdo whatsapp-setup --instance <deploy-id> --phone-number "+6512345678"
clawmacdo whatsapp-setup --instance <deploy-id> --phone-number "+6512345678" --reset  # reset + setup in one SSH session
clawmacdo whatsapp-qr --instance <deploy-id>   # re-fetch QR if expired
clawmacdo whatsapp-reset --instance <deploy-id> # clear session, force new QR
# Lightsail/Azure instances automatically use their default SSH users for WhatsApp repair/QR.
# The web UI QR fetch now ignores a missing prior login process instead of failing with an empty SSH error.

# OpenClaw version management
clawmacdo openclaw-versions                       # list available versions
clawmacdo openclaw-versions --json                # JSON output
clawmacdo openclaw-install --instance <deploy-id> --version 2026.3.22  # pin version
clawmacdo deploy --provider digitalocean --openclaw-version 2026.3.22 ...  # deploy with pinned version

# Change AI model on a running instance
clawmacdo update-model --instance <deploy-id> \
  --primary-model openai --openai-key "$OPENAI_API_KEY"

# Install a plugin
clawmacdo plugin-install --instance <deploy-id> --plugin "@openguardrails/moltguard"

# Refresh IP after instance restart
clawmacdo update-ip --instance <deploy-id>

# Enable Tailscale Funnel (public HTTPS access)
clawmacdo tailscale-funnel --instance <deploy-id> --auth-key "$TAILSCALE_AUTH_KEY"
clawmacdo funnel-on --instance <deploy-id>

# Create and restore snapshots
clawmacdo do-snapshot --do-token "$DO_TOKEN" --droplet-id 12345 --snapshot-name "backup"
clawmacdo do-restore --do-token "$DO_TOKEN" --snapshot-name "backup"

# Destroy an instance
clawmacdo destroy --provider digitalocean --do-token "$DO_TOKEN" --name "openclaw-abc123"

# Start the web UI
clawmacdo serve --port 3456
```

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

### Update AI Model on a Running Instance

Change the primary AI model or failover chain on a deployed OpenClaw instance without redeploying.

```bash
# Switch to OpenAI as primary
clawmacdo update-model --instance <deploy-id> \
  --primary-model openai --openai-key "$OPENAI_API_KEY"

# Switch to BytePlus ARK with Anthropic failover
clawmacdo update-model --instance <deploy-id> \
  --primary-model byteplus --failover-1 anthropic \
  --byteplus-ark-api-key "$BYTEPLUS_ARK_API_KEY" \
  --anthropic-key "$ANTHROPIC_API_KEY"

# Multi-model failover chain
clawmacdo update-model --instance <deploy-id> \
  --primary-model anthropic --failover-1 openai --failover-2 gemini \
  --anthropic-key "$ANTHROPIC_API_KEY" \
  --openai-key "$OPENAI_API_KEY" --gemini-key "$GEMINI_API_KEY"
```

The command updates API keys in `.env`, configures provider settings (BytePlus `openclaw.json`), sets the model via `openclaw models set`, adds failovers, and restarts the gateway service. API keys are optional — if omitted, the existing key on the instance is preserved.

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

### Create a DigitalOcean Snapshot

Create a named snapshot from an existing DigitalOcean droplet. Optionally shuts down the droplet first for a clean snapshot.

```bash
# Create a snapshot (droplet stays running)
clawmacdo do-snapshot \
  --do-token "$DO_TOKEN" \
  --droplet-id 558765268 \
  --snapshot-name "my-openclaw-2026-03-19"

# Recommended: shut down first for a clean snapshot, then power back on
clawmacdo do-snapshot \
  --do-token "$DO_TOKEN" \
  --droplet-id 558765268 \
  --snapshot-name "my-openclaw-2026-03-19" \
  --power-off
```

### Restore a DigitalOcean Droplet from Snapshot

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

### Create a BytePlus Snapshot

Create a named snapshot of a BytePlus ECS instance's system disk.

```bash
clawmacdo bp-snapshot \
  --instance-id i-abc123 \
  --snapshot-name "my-openclaw-backup"
```

### Restore a BytePlus ECS Instance from Snapshot

Create a new instance from an existing BytePlus snapshot. This creates a custom image from the snapshot, then launches a new instance from that image.

```bash
# Restore from a snapshot by name
clawmacdo bp-restore \
  --snapshot-name "my-openclaw-backup"

# With spot instance for cost savings
clawmacdo bp-restore \
  --snapshot-name "my-openclaw-backup" \
  --size ecs.g3i.large \
  --spot
```

### Create a Lightsail Snapshot

Create a snapshot of an AWS Lightsail instance.

```bash
clawmacdo ls-snapshot \
  --instance-name "openclaw-abc123" \
  --snapshot-name "my-openclaw-backup" \
  --region ap-southeast-1
```

### Restore a Lightsail Instance from Snapshot

Create a new instance directly from an existing Lightsail snapshot.

```bash
# Restore from a snapshot by name
clawmacdo ls-restore \
  --snapshot-name "my-openclaw-backup" \
  --region ap-southeast-1

# With size override
clawmacdo ls-restore \
  --snapshot-name "my-openclaw-backup" \
  --size s-4vcpu-8gb
```

### Destroy an Instance

Delete an instance by name across any supported provider. Removes cloud SSH key, local key, and (for BytePlus) EIP and VPC resources.

```bash
# DigitalOcean
clawmacdo destroy \
  --provider digitalocean \
  --do-token "$DO_TOKEN" \
  --name "openclaw-abc123"

# Tencent Cloud (skip confirmation prompt)
clawmacdo destroy \
  --provider tencent \
  --tencent-secret-id "$TENCENT_SECRET_ID" \
  --tencent-secret-key "$TENCENT_SECRET_KEY" \
  --name "openclaw-abc123" \
  --yes

# BytePlus (also releases EIP and VPC resources)
clawmacdo destroy \
  --provider byteplus \
  --byteplus-access-key "$BYTEPLUS_ACCESS_KEY" \
  --byteplus-secret-key "$BYTEPLUS_SECRET_KEY" \
  --name "openclaw-abc123"
```

### Snapshot/Restore Progress Tracking

Snapshot and restore operations return an `operation_id` immediately and run asynchronously. Track progress via SSE:

```bash
# Start a snapshot (returns operation_id immediately)
curl -X POST http://localhost:3456/api/deployments/{id}/snapshot \
  -H 'Content-Type: application/json' \
  -d '{"snapshot_name": "my-backup", "do_token": "$DO_TOKEN"}'
# Response: {"ok": true, "message": "Snapshot operation started.", "operation_id": "abc-123"}

# Start a restore (returns operation_id immediately)
curl -X POST http://localhost:3456/api/snapshots/restore \
  -H 'Content-Type: application/json' \
  -d '{"provider": "digitalocean", "snapshot_name": "my-backup", "do_token": "$DO_TOKEN"}'
# Response: {"ok": true, "message": "Restore operation started.", "operation_id": "def-456"}

# Stream progress via SSE
curl -N http://localhost:3456/api/deploy/{operation_id}/events
# SSE messages include [Step N/T] for progress bars
# Terminal: SNAPSHOT_COMPLETE_JSON:{...} or RESTORE_COMPLETE_JSON:{...}
# Error: SNAPSHOT_ERROR:... or RESTORE_ERROR:...
```

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

The login screen uses a 6-digit PIN gate and shows invalid PIN attempts as an inline alert on the page.

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

## Programmatic Usage (Node.js)

The npm package exports `getBinaryPath()` so you can call clawmacdo from Node.js scripts or automation tools.

```bash
npm install clawmacdo
```

```javascript
const { execSync, spawn } = require("child_process");
const { getBinaryPath } = require("clawmacdo");

const bin = getBinaryPath(); // absolute path to the clawmacdo binary

// --- Deploy a new instance ---
const deploy = execSync(`${bin} deploy \
  --provider lightsail \
  --customer-name "my-openclaw" \
  --customer-email "you@example.com" \
  --aws-access-key-id "${process.env.AWS_ACCESS_KEY_ID}" \
  --aws-secret-access-key "${process.env.AWS_SECRET_ACCESS_KEY}" \
  --anthropic-key "${process.env.ANTHROPIC_API_KEY}" \
  --primary-model anthropic \
  --json`, { encoding: "utf8" });
console.log(JSON.parse(deploy));

// --- Track deploy progress (streaming) ---
const track = spawn(bin, ["track", "<deploy-id>", "--follow", "--json"]);
track.stdout.on("data", (chunk) => {
  console.log("progress:", chunk.toString());
});

// --- Set up Telegram bot ---
execSync(`${bin} telegram-setup \
  --instance <deploy-id> \
  --bot-token "${process.env.TELEGRAM_TOKEN}"`, { stdio: "inherit" });

// --- Set up WhatsApp (displays QR code) ---
execSync(`${bin} whatsapp-setup \
  --instance <deploy-id> \
  --phone-number "+6512345678"`, { stdio: "inherit" });

// --- Fetch WhatsApp QR code ---
const qr = execSync(`${bin} whatsapp-qr --instance <deploy-id>`, { encoding: "utf8" });
console.log(qr); // ASCII QR code

// --- Change AI model ---
execSync(`${bin} update-model \
  --instance <deploy-id> \
  --primary-model openai \
  --openai-key "${process.env.OPENAI_API_KEY}"`, { stdio: "inherit" });

// --- Install a plugin ---
execSync(`${bin} plugin-install \
  --instance <deploy-id> \
  --plugin "@openguardrails/moltguard"`, { stdio: "inherit" });

// --- Refresh IP after restart ---
execSync(`${bin} update-ip --instance <deploy-id>`, { stdio: "inherit" });

// --- Create snapshot ---
execSync(`${bin} do-snapshot \
  --do-token "${process.env.DO_TOKEN}" \
  --droplet-id 12345 \
  --snapshot-name "my-backup"`, { stdio: "inherit" });

// --- Restore from snapshot ---
execSync(`${bin} do-restore \
  --do-token "${process.env.DO_TOKEN}" \
  --snapshot-name "my-backup"`, { stdio: "inherit" });

// --- Destroy an instance ---
execSync(`${bin} destroy \
  --provider digitalocean \
  --do-token "${process.env.DO_TOKEN}" \
  --name "openclaw-abc123" --yes`, { stdio: "inherit" });

// --- Start the web UI programmatically ---
const server = spawn(bin, ["serve", "--port", "3456"], { stdio: "inherit" });
```

### TypeScript

```typescript
import { getBinaryPath } from "clawmacdo";
import { execSync } from "child_process";

const bin: string = getBinaryPath();
execSync(`${bin} deploy --provider lightsail ...`, { stdio: "inherit" });
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

## Skills Data API

The `skills-data-api/` directory contains a standalone Node.js/Express service for browsing and serving Claude Code skill marketplace data, backed by MongoDB.

```bash
# Install dependencies
cd skills-data-api
npm install

# Load skills data into MongoDB
pwsh -File ./load-mongo.ps1

# Start the API server
node index.js
```

Operational repo scripts now ship with PowerShell entrypoints under `scripts/*.ps1` and can be run cross-platform with `pwsh -File`.

### Docker

```bash
cd skills-data-api
docker build -t skills-data-api .
docker run -p 3000:3000 \
  -e MONGODB_URI="mongodb://host.docker.internal:27017/skills" \
  skills-data-api
```

### Key Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/skills` | List all skills (paginated) |
| `GET` | `/api/skills/:name` | Get a skill by name |
| `GET` | `/api/skills/search?q=...` | Search skills by keyword |

See [`skills-data-api/README.md`](skills-data-api/README.md) for full API documentation.

## Project Structure

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
├── skills-data-api/        # 🧠  Node.js skills marketplace API (MongoDB)
├── e2e/                    # 🧪  Playwright end-to-end test suite
├── assets/                 # Static assets (mascot, etc.)
└── docs/                   # Design docs and usage reference
```

### Crate Overview

| Crate | Purpose | Dependencies |
|-------|---------|--------------|
| **clawmacdo-cli** | Main binary, command parsing, orchestration | All other crates |
| **clawmacdo-core** | Configuration, errors, shared types | Minimal (serde, anyhow) |
| **clawmacdo-cloud** | DigitalOcean, AWS Lightsail, Tencent Cloud & BytePlus APIs | reqwest, async-trait |
| **clawmacdo-provision** | Server setup, package installation | SSH, Core, UI |
| **clawmacdo-db** | SQLite operations, job tracking | rusqlite |
| **clawmacdo-ssh** | SSH connections, file transfers | ssh2 |
| **clawmacdo-ui** | Progress bars, web interface | indicatif, axum |

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

## Security Hardening

- Privileged remote provisioning commands now run through stdin-fed shells instead of nested quoted `sudo` / `su -c` wrappers.
- User-supplied hostnames are normalized and validated before any deploy flow uses them.
- The web UI now only accepts backup archives from `~/.clawmacdo/backups` and SSH keys from `~/.clawmacdo/keys`.
- Backup restore validates the local `.tar.gz` before upload and extracts remotely with `--no-same-owner` and `--no-same-permissions` into a dedicated restore directory.
- The gateway service now reads `~/.openclaw/gateway.env` instead of the broader `.env`, so setup-only secrets such as `ANTHROPIC_SETUP_TOKEN` are not inherited by the long-running service.
- Direct Docker-group access for `openclaw` has been removed. If sandbox mode is requested during deploy, the deploy now forces sandbox mode off until a safer non-root mediation path exists.
- Lightsail credentials are passed only to the child AWS CLI processes instead of mutating process-global environment variables or writing `~/.aws/credentials`.
- Tencent's optional security-group helper now takes SSH ingress from `CLAWMACDO_TENCENT_SSH_CIDR` and defaults to `127.0.0.1/32` instead of opening SSH to the world.

See [docs/HIGH_SECURITY_FIXES.md](docs/HIGH_SECURITY_FIXES.md) for the finding-by-finding code map, rationale, and functionality impact.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Run `cargo clippy` and `cargo test`
5. Submit a pull request

## License

Copyright (c) 2026 Kenneth Phang

This software is licensed under a dual license model:

1. **GNU General Public License v3.0** — for open source use
   See [LICENSE-GPL.md](LICENSE-GPL.md) for details.

2. **Commercial License** — for proprietary/commercial use
   See [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md) for details.

For licensing inquiries, contact: bunnyppl@gmail.com

## Documentation

| Document | Description |
|----------|-------------|
| [CLI Usage Guide](docs/clawmacdo_usage.md) | Complete reference for all subcommands with examples and sample responses |
| [Deployment Architecture](docs/DEPLOYMENT_ARCHITECTURE_RESEARCH.md) | Cloud provider architecture research and design decisions |
| [Codebase Logic & Data Flow](docs/CODEBASE_LOGIC_AND_DATA_FLOW.md) | End-to-end logic flow, module boundaries, and data structures |
| [Tracking Architecture](docs/TRACKING_ARCHITECTURE.md) | Deploy/snapshot/restore progress tracking system design |
| [TanStack Progress Tracking](docs/tanstack-progress-tracking.md) | Frontend integration guide for TanStack (React Query) progress bars |
| [Security Scan](docs/SECURITY_SCAN.md) | Security scanning CLI and vulnerability assessment |
| [Security Flaw Evaluation](docs/EVAL_SECURITY_FLAW.md) | Security flaw evaluation report and findings |
| [High Security Fixes](docs/HIGH_SECURITY_FIXES.md) | Code-level remediation map for all HIGH findings |
| [Tencent Cloud Plan](docs/TENCENT_PLAN.md) | Tencent Cloud provider support plan |
| [Repository Guidelines](docs/AGENTS.md) | Contribution guidelines and repository conventions |

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for version history and release notes.

---

**Current version:** 0.55.0


