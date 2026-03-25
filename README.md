# clawmacdo

[![Release](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml)
[![Changelog](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml)

Rust CLI tool for deploying [OpenClaw](https://openclaw.ai) to **DigitalOcean**, **AWS Lightsail**, **Tencent Cloud**, **Microsoft Azure**, or **BytePlus Cloud** — with Claude Code, Codex, and Gemini CLI pre-installed.

## ✨ What's New in v0.48.0

- **`skill-remove` subcommand** — delete a deployed skill by name from an instance's workspace and restart the gateway (`--instance` + `--skill`)
- **`skill-list` subcommand** — list all skill directories on an instance with their gateway-registered name and readiness status
- **`skill-check-perms` subcommand** — audit file ownership and permissions for a deployed skill; add `--fix` to auto-correct to `openclaw:openclaw` / `644`/`755`

## What's New in v0.48.0

- **`skill-diff` subcommand** — compare a local skill directory against the deployed skill on an instance; reports files that are in-sync (✓), modified (≠), new locally (+), or only on instance (−); also shows gateway skill status

## What's New in v0.46.4

- **`cron-message` subcommand** — schedule a recurring message to the OpenClaw gateway agent; the agent processes it and delivers the response to Telegram, WhatsApp, or any other connected channel (uses `openclaw cron add` under the hood)
- **`cron-tool` subcommand** — schedule recurring tool execution on a deployed instance; the agent runs the named tool and announces the result to the chosen channel
- **`cron-list` subcommand** — list all cron jobs on a deployed instance
- **`cron-remove` subcommand** — remove a cron job by name from a deployed instance
- **`skill-deploy` subcommand** — upload a `.zip` of OpenClaw skills to a deployed instance; extracts into `~/.openclaw/workspace/skills/` and restarts the gateway automatically

## What's New in v0.38.0

- **No spurious CLI warnings on non-deploy commands** — `telegram-setup`, `telegram-pair`, and other SSH-only commands no longer print "Azure/AWS CLI not found" at startup; provider CLI checks now run only when deploying to that provider
- **`telegram-setup` `gateway.env` fix** — bot token is now written to both `.env` and `gateway.env`; the systemd gateway service loads `gateway.env` via `EnvironmentFile`, so previously a re-run with a new token left the running service polling with the old bot
- **`telegram-setup` resets pairing state** — clears previous bot's pairing credentials and update offsets before reconfiguring, giving users a clean pairing flow with the new bot

## What's New in v0.46.4

- **SSH performance** — `telegram-setup` and `whatsapp-setup` reuse a single SSH session for all 4 steps (one TCP connect + handshake instead of four); cipher negotiation now prefers faster AEAD ciphers (`chacha20-poly1305`, `aes128-gcm`); ephemeral deploy keys use RSA-2048 instead of RSA-4096 (~4× faster key generation); `wait_for_ssh` no longer probes wrong users on Lightsail (`ubuntu`) and Azure (`azureuser`)
- **Telegram/WhatsApp Lightsail fix** — `telegram-setup`, `telegram-pair`, `whatsapp-setup`, and `whatsapp-qr` now SSH as `ubuntu` (not `root`) on Lightsail instances
- **`update-model` subcommand** — change the AI model on a running OpenClaw instance without redeploying (updates API keys, provider config, model settings, and restarts the gateway)
- **`update-ip` subcommand** — refresh the IP address of a deployed instance from the cloud provider API (Lightsail, DigitalOcean, BytePlus) and update both JSON deploy record and SQLite
- **Refresh IP button** — new "Refresh IP" button in Deployments tab queries the cloud provider and updates the IP in-place
- **Deployments action dropdown** — deployment row actions now open in a stacked menu so controls stay readable instead of overlapping in narrow tables
- **Deployments table fit** — deployments table now uses a tighter fixed layout with wrapped cell content to avoid left-right scrolling in the tab
- **Funnel actions in dropdown** — the Deployments tab now handles the two-step funnel flow from the Actions menu: first toggle funnel on/off, then open the funnel URL once it becomes available
- **Snapshot/restore progress tracking** — snapshot and restore operations are now async with step-by-step progress via SSE; the frontend can display real-time progress bars using `GET /api/deploy/{operation_id}/events`
- **Deploy progress in Deployments tab** — running deployments show an animated progress bar with current step label, polling every 3 seconds
- **Funnel verification** — toggling funnel ON now polls the funnel status with a progress bar before showing the Open button
- **Docker fix: systemd user manager restart** — "Fix Agent Docker Access" now restarts the systemd user service manager so the gateway picks up the docker group
- **`KillMode=control-group`** — gateway service now kills the entire cgroup on restart, preventing orphaned child processes from holding the port
- **AWS credential passthrough** — web UI credentials are written to `~/.aws/credentials` so the AWS CLI uses them instead of stale local config
- **Lightsail destroy with credentials** — destroy modal now prompts for AWS Access Key ID and Secret Access Key
- **Lightsail snapshot listing** — credentials from the web UI are now passed through to the AWS CLI for snapshot listing
- **`whatsapp-setup` subcommand** — set up WhatsApp on a deployed instance (set phone number, enable plugin, restart gateway, fetch pairing QR code)
- **`whatsapp-qr` subcommand** — fetch the WhatsApp pairing QR code from a deployed instance (re-fetch if expired)
- **`plugin-install` subcommand** — install OpenClaw plugins on deployed instances via `clawmacdo plugin-install --instance <id> --plugin @openguardrails/moltguard` (installs via pnpm, enables plugin, restarts gateway)
- **Windows PowerShell scripts** — all shell scripts now have `.ps1` equivalents for Windows support (`release.ps1`, `npm-package.ps1`, `npm-publish.ps1`, scan scripts, etc.)
- **Agent Docker Access warning** — deploy form shows the common Docker socket permission error with a clear fix instruction
- **Dual license** — switched from MIT to GPLv3 (open source) + Commercial (proprietary) dual license model

### Previous highlights (v0.25.x – v0.26.x)
- **`do-snapshot` subcommand** — create a named DigitalOcean snapshot from an existing droplet by ID, with optional `--power-off` flag for clean shutdown/snapshot/power-on cycle
- **BytePlus EIP cost reduction** — switched from pay-by-bandwidth to pay-by-traffic billing, reduced default bandwidth from 10 Mbps to 5 Mbps
- **BytePlus spot instances** — new `--spot` flag on deploy enables `SpotAsPriceGo` strategy for up to ~80% compute cost savings
- **`bp-snapshot` / `bp-restore`** — snapshot and restore for BytePlus ECS instances
- **`ls-snapshot` / `ls-restore`** — snapshot and restore for AWS Lightsail instances
- **BytePlus EIP orphan cleanup** — destroy command now finds and releases unbound EIPs left behind after instance termination

### Previous highlights (v0.21.x – v0.23.x)
- **`destroy` subcommand** — delete any openclaw instance across all 5 cloud providers with interactive confirmation
- **`skills-data-api` service** — Node.js/Express API backed by MongoDB for browsing, scraping, and serving Claude Code skill marketplace data
- **Playwright e2e test suite** — CSV-driven deploy form testing under `e2e/`, covering all 5 cloud providers with 30+ scenarios

### Previous highlights (v0.20.x)
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
├── skills-data-api/        # 🧠  Node.js skills marketplace API (MongoDB)
├── e2e/                    # 🧪  Playwright end-to-end test suite
├── assets/                 # Static assets (mascot, etc.)
└── docs/                   # Design docs and usage reference
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
- **Snapshot create**: create a named snapshot from an existing DigitalOcean droplet, with optional power-off for data consistency
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
clawmacdo telegram-pair --instance <deploy-id> --code <PAIRING_CODE>

# Set up WhatsApp (displays QR code to scan)
clawmacdo whatsapp-setup --instance <deploy-id> --phone-number "+6512345678"
clawmacdo whatsapp-qr --instance <deploy-id>   # re-fetch QR if expired

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

### Create a DigitalOcean Snapshot from a Droplet

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

The command verifies the droplet exists, optionally shuts it down, creates the snapshot, polls until complete, confirms the snapshot, and optionally powers the droplet back on.

### Create a BytePlus Snapshot from an ECS Instance

Create a named snapshot of a BytePlus ECS instance's system disk.

```bash
clawmacdo bp-snapshot \
  --instance-id i-abc123 \
  --snapshot-name "my-openclaw-backup"
```

### Restore a BytePlus ECS Instance from a Snapshot

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

### Restore a Lightsail Instance from a Snapshot

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

Security scan scripts always write their main outputs to the system temp directory and only mirror them into `/root/.openclaw/workspace` when that directory is accessible.
| [High Security Fixes](docs/HIGH_SECURITY_FIXES.md) | Code-level remediation map for all HIGH findings |
| [Tencent Cloud Plan](docs/TENCENT_PLAN.md) | Tencent Cloud provider support plan |
| [Repository Guidelines](docs/AGENTS.md) | Contribution guidelines and repository conventions |

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for version history and breaking changes.

---

**Last updated:** March 19, 2026
**Current version:** 0.48.0
**Architecture version:** 2.0 (modular workspace)


















