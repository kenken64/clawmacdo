# clawmacdo

<p align="center">
  <img src="clawmacdo.png" alt="clawmacdo preview" width="900" />
</p>

[![Release](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml)
[![Changelog](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml)

Rust CLI and web UI for deploying [OpenClaw](https://openclaw.ai) to **DigitalOcean**, **AWS Lightsail**, **Tencent Cloud**, **Microsoft Azure**, or **BytePlus Cloud**, and [Hermes Agent](https://hermes-agent.nousresearch.com/) natively to **AWS Lightsail**. OpenClaw deployments include Claude Code, Codex, and Gemini CLI; Hermes supports OpenRouter or AWS Bedrock, OpenCode CLI v2, Telegram, and an optional dashboard.

## Features

- **Multi-cloud**: Deploy to DigitalOcean, AWS Lightsail, Tencent Cloud, Microsoft Azure, or BytePlus Cloud with `--provider` flag
- **Hermes Agent on Lightsail**: `hermes-provision` or the web UI installs a verified native Ubuntu 24.04 bundle alongside required OS packages, starts Hermes, and queues the full OS upgrade after deployment
- **Hermes messaging and dashboard**: authorize Telegram users by ID and optionally expose the password-protected dashboard through a Cloudflare Quick Tunnel
- **1-click deploy**: generate SSH keys, provision a cloud instance, install Node 24 + OpenClaw + Claude Code + Codex + Gemini CLI + OpenCode, restore config, configure `.env` (API + messaging), start the gateway, and auto-configure model failover
- **Cloud-to-cloud migration**: SSH into a source instance, back up remotely, deploy to a new instance, restore
- **Snapshot & restore**: create and restore named snapshots for DigitalOcean, BytePlus, and AWS Lightsail
- **Destroy**: delete an instance by name with confirmation, clean up SSH keys (cloud + local)
- **Status**: list all openclaw-tagged instances with IPs
- **Backup**: back up local `~/.openclaw/` config into a timestamped `.tar.gz`
- **Web UI**: browser-based deploy interface with a lobster-red OpenClaw-inspired theme and real-time SSE progress streaming (optional)
- **Security groups**: auto-create firewall rules on Tencent Cloud and BytePlus (SSH + HTTP/HTTPS + Gateway)

## Supported Cloud Providers

The provider flags below apply to OpenClaw. For Hermes Agent, use [`hermes-provision`](#provision-hermes-agent-on-aws-lightsail) or select **Hermes Agent** in the web UI; Hermes provisioning currently targets AWS Lightsail.

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
clawmacdo whatsapp-status --instance <deploy-id>               # check channel status via Gateway API
clawmacdo whatsapp-wait --instance <deploy-id>                 # poll until connected (default 120s)
clawmacdo whatsapp-wait --instance <deploy-id> --timeout 60    # custom timeout
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

# Update the Bedrock token used by the OpenClaw workspace tty proxy
clawmacdo bedrock-token-set --instance <deploy-id> \
  --bearer-token "$AWS_BEARER_TOKEN_BEDROCK"

# Set OPENCLAW_INSTANCE in the claw-ttyproxy project .env
clawmacdo ttyproxy-instance-set --instance <deploy-id> \
  --openclaw-instance openclaw-prod-1

# Set OpenClaw display name and owner context
clawmacdo openclaw-identity --instance <deploy-id> \
  --openclaw-name "Clawdia" --owner-name "Kenneth"

# Update the Gyne consumer profile in workspace/gyne-agent/.env
clawmacdo gyne-consumer-profile --instance <deploy-id> --name consumer-4

# Download OpenClaw Markdown context files and memory logs as a ZIP
clawmacdo openclaw-md-download --instance <deploy-id> --output ~/backups/

# Project wiki read/write/export for web apps
clawmacdo wiki-tree --instance <deploy-id> --project llm_wiki --json
clawmacdo wiki-read --instance <deploy-id> --path llm_wiki/INDEX.md --json
clawmacdo wiki-write --instance <deploy-id> --path llm_wiki/INDEX.md \
  --content-file /tmp/INDEX.md --base-sha <sha-from-read> --json
clawmacdo wiki-ingest --instance <deploy-id> --project wiki-163327 \
  --source /tmp/uploaded-doc.md \
  --prompt "Decide where this belongs and update related pages." --json
clawmacdo wiki-export --instance <deploy-id> --project llm_wiki --json

# Regenerate the OpenClaw gateway token
clawmacdo openclaw-gateway-token --instance <deploy-id>
clawmacdo openclaw-gateway-url --instance <deploy-id> --json

# Configure Remotion avatar app and return a Cloudflare Quick Tunnel URL
clawmacdo remotion-avatar-setup --instance <deploy-id> --name "Kenny" \
  --openai-api-key "$OPENAI_API_KEY" --voice-gender female \
  --avatar-glb ./kenny_avatar.glb --json

# Install a plugin
clawmacdo plugin-install --instance <deploy-id> --plugin "@openguardrails/moltguard"

# Google Workspace (gws) authentication
# gws auth login is an interactive browser OAuth flow with no headless mode, and
# the instance is headless — so credentials are injected, not minted on the box.
# Mode 1 — push a credentials JSON exported elsewhere (browser machine / 2ndbrain.ceo):
clawmacdo gws-login --instance <deploy-id> --credentials ./gws-credentials.json
clawmacdo gws-login --instance <deploy-id> --credentials ./token.json --filename token.json  # custom dest name
# Mode 2 — hand clawmacdo a raw OAuth authorization code; it exchanges it with Google
# and writes the authorized_user credentials. Add --code-verifier if the flow used PKCE.
# The auth URL must request access_type=offline, or Google returns no refresh_token.
clawmacdo gws-login --instance <deploy-id> \
  --code <oauth-code> \
  --client-id <client-id> --client-secret <client-secret> \
  --redirect-uri <redirect-uri> [--code-verifier <pkce-verifier>]
clawmacdo gws-logout --instance <deploy-id>   # gws auth logout (revoke) + clear local credentials

# Refresh IP after instance restart
clawmacdo update-ip --instance <deploy-id>

# Enable Tailscale Funnel (public HTTPS access)
# Performs clean Tailscale registration (wipes stale state if hostname mismatch)
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

### Provision Hermes Agent on AWS Lightsail

Provision a Lightsail instance and bootstrap [Hermes Agent](https://hermes-agent.nousresearch.com/) with OpenRouter,
OpenAI GPT-6 Sol, and the OpenCode CLI v2 preinstalled. Hermes runs natively as the `ubuntu` user with the local terminal backend; provisioning does not install Docker.

#### Prebuilt installation

Prebuilt installation is automatic on **Ubuntu 24.04 amd64 and arm64**. Each deployment creates an
instance in your own AWS account and downloads the matching software archive from the published
[Hermes runtime bundle](https://github.com/kenken64/clawmacdo/releases/tag/hermes-bundle-v0.22.5-1).
You do not need a shared Lightsail snapshot or an extra CLI flag.

| Bundled component | Pinned version |
|-------------------|----------------|
| Runtime bundle | `hermes-bundle-v0.22.5-1` |
| Hermes Agent and compiled dashboard | `0.22.5` |
| OpenCode CLI | `2.1.3` |
| cloudflared | `2026.10.3` |

1. `apt-get update` and required runtime-library installation run alongside bundle download, verification,
   and extraction. The archive includes the Python runtime, dependencies, and native tools, so a
   successful bundle install avoids building Hermes and its dashboard on every instance.
2. Both jobs must finish successfully before service setup. Step 7 streams separate `OS:` and
   `Bundle:` progress messages in the CLI and web UI.
3. Provisioning writes your credentials and settings, configures the selected services, and checks
   readiness. OpenCode and cloudflared are activated only when their options are enabled.
4. After step 8 saves the deployment record, a short SSH call queues the full `apt-get upgrade`
   as a background systemd job. **Hermes is ready to use while the OS upgrade runs.** The upgrade
   continues if you close the CLI or browser; deployment does not wait for it to finish.

Bundles are built on clean CI runners and verified on separate fresh amd64/arm64 VMs with external
networking disabled for runtime tests. They contain software only: API keys, Telegram settings,
dashboard passwords, SSH keys, and user sessions are configured separately.

The installer checks the platform, pinned release and source commit, archive size, SHA-256, and
extraction paths. Only a missing release asset (**HTTP 404**) or an unsupported bundle platform allows
fallback to pinned Hermes source, after OS setup finishes. Checksum mismatches, invalid manifests,
unsafe archives, and other download or installation failures stop provisioning.

Full OS upgrades remain enabled but finish separately from deployment. The summary reports whether
the upgrade was queued and provides status/log commands. If the SSH handoff cannot be confirmed,
Hermes remains ready and the summary explains how to check and retry the job. Background OS upgrades
are available starting with **v0.98.0**.

Download and extraction time still depend on the connection and instance size; source fallback
still incurs the longer install. Deferring the OS upgrade does not eliminate bundle installation time.
See the [bundle build and verification guide](scripts/hermes-bundle/README.md) for release details.

To check background OS patching on a deployment using the new flow:

```bash
ssh -i <ssh-key-path> ubuntu@<instance-ip> 'cat /var/lib/clawmacdo/hermes-os-upgrade/status'
ssh -i <ssh-key-path> ubuntu@<instance-ip> 'sudo journalctl -u clawmacdo-hermes-os-upgrade -f'
# Retry an upgrade whose launch or execution failed:
ssh -i <ssh-key-path> ubuntu@<instance-ip> 'sudo systemctl start --no-block clawmacdo-hermes-os-upgrade.service'
```

The job reports `running`, `completed`, a required reboot, or failure. It preserves existing package
configuration files and uses needrestart's list-only mode. No automatic reboot is performed;
package installation scripts can still restart their own services.

#### CLI and web UI setup

```bash
export AWS_ACCESS_KEY_ID="your_access_key_id"
export AWS_SECRET_ACCESS_KEY="your_secret_access_key"
export OPENROUTER_API_KEY="sk-or-..."

clawmacdo hermes-provision \
  --aws-access-key-id "$AWS_ACCESS_KEY_ID" \
  --aws-secret-access-key "$AWS_SECRET_ACCESS_KEY" \
  --aws-region ap-southeast-1 \
  --openrouter-api-key "$OPENROUTER_API_KEY" \
  --openrouter-model openai/gpt-6-sol \
  --telegram-bot-token "$TELEGRAM_BOT_TOKEN" \
  --telegram-allowed-users 123456789
```

The AWS IAM credentials can also be supplied through the `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`
environment variables shown above.

The same setup is available in the web UI (`clawmacdo serve --port 3456`):

1. Unlock the UI and select **Hermes Agent**.
2. Enter your AWS Lightsail credentials, region, and instance size.
3. Keep **OpenRouter** selected, enter your API key, and choose the model (default `openai/gpt-6-sol`).
4. Leave **Install OpenCode CLI v2** checked to install it with the same OpenRouter key and model.
5. Enter your Telegram bot token from @BotFather and your numeric Telegram user ID in **Allowed User IDs**, then click **Provision Hermes Agent**.
6. After provisioning, message the bot from an allowed account. No pairing code or approval step is needed.

The web UI keeps AWS Bedrock available as an alternative AI provider.
OpenCode uses the bundled v2 binary when available, or the official v2 installer, and reads its OpenRouter key from a file owned by `ubuntu` with `0600` permissions,
so authentication also works in noninteractive sessions.

What gets configured:

- The Telegram bot token is validated before provisioning and verified with Telegram's `getMe` API when the network allows.
- Hermes `config.yaml` uses `provider: openrouter` with `default: openai/gpt-6-sol`.
- `OPENROUTER_API_KEY` and Telegram settings are written to `/home/ubuntu/.hermes/.env` with `0600` permissions.
- `hermes-gateway.service` starts Hermes at boot. View logs with `sudo journalctl -u hermes-gateway -f`.
- `--dashboard` also enables `hermes-dashboard.service` on `127.0.0.1:9119`, accessible through an SSH tunnel.
- OpenCode CLI v2 is installed for the `ubuntu` user and configured with the **same** `--openrouter-model`
  (`openrouter/openai/gpt-6-sol`) in `~/.config/opencode/opencode.json`.
- Use `--no-opencode` to skip the OpenCode CLI install.

#### Dashboard and Cloudflare Quick Tunnel

To give the dashboard an HTTPS address, enable **Cloudflare Quick Tunnel for the dashboard** in the
web UI. No Cloudflare account, tunnel token, custom hostname, or DNS setup is needed. Enter a dashboard
password of at least 12 characters; sign in to Hermes as **admin** using that password.

For the CLI, add `--cloudflare-tunnel` to your provisioning command. This also enables the dashboard.
AWS and OpenRouter credentials are read from the environment as above:

```bash
export HERMES_DASHBOARD_PASSWORD="your_long_dashboard_password"
clawmacdo hermes-provision --cloudflare-tunnel
```

The password can also be supplied with `--dashboard-password`. It protects the Hermes dashboard;
Cloudflare Quick Tunnel itself needs no credentials. Provisioning installs native `cloudflared` and runs
`cloudflared tunnel --url http://127.0.0.1:9119` as `hermes-cloudflared.service` at boot. Hermes stays on
localhost with no new inbound firewall ports, and its password is stored as a scrypt hash.

The generated `https://<random>.trycloudflare.com` link appears in the completed deployment summary and
saved record. **The URL changes when the tunnel restarts.** Each connector start updates Hermes with the
new hostname, restarts the dashboard, and checks authentication and the Cloudflare connection before saving
the current URL. The web UI retains the last reported link; retrieve the current link after a restart with:

```bash
ssh -i <ssh-key-path> ubuntu@<instance-ip> 'cat /var/lib/hermes-cloudflared/dashboard-url'
```

View connector logs with `sudo journalctl -u hermes-cloudflared -f`. The web UI does not save the dashboard
password in browser storage. `--dry-run` redacts the password and leaves the dashboard URL empty because
no tunnel is created. [Cloudflare Quick Tunnels](https://developers.cloudflare.com/tunnel/get-started/quick-tunnels/)
are temporary, have no uptime guarantee, and do not support Server-Sent Events (SSE).

#### Telegram access and AI providers

Telegram access is configured with `--telegram-allowed-users 123456789,987654321` (or
`TELEGRAM_ALLOWED_USERS`). These users can message the bot directly once provisioning finishes.
Use `--telegram-home-channel` to choose the notification destination; it defaults to the first allowed
user and does not grant access by itself. Omitting allowed users leaves Hermes' own access controls in
place. Clawmacdo no longer asks for a pairing code in `hermes-provision` or the web UI, and the
`hermes-telegram-pair` subcommand has been removed.

The AI provider is selected automatically: an OpenRouter key takes precedence, otherwise the command falls back to
AWS Bedrock. Override with `--ai-provider openrouter` or `--ai-provider bedrock`. Bedrock still accepts
`--bedrock-api-key`, `--bedrock-region`, and `--bedrock-model`.

Use `--dry-run` to print the generated Lightsail user-data script without creating resources, or `--json` for
structured output.

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

Set a primary AI model and optional failovers for the deployed instance. Supported models: `anthropic`, `openai`, `gemini`, `byteplus`, `opencode`.

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

# OpenCode with MiniMax M2.5 Free as primary (no API key required)
clawmacdo deploy --provider do --customer-email "user@example.com" \
  --primary-model opencode
```

| Model | `--primary-model` value | Model identifier | Required flag |
|-------|------------------------|------------------|---------------|
| Anthropic Claude | `anthropic` | `anthropic/claude-opus-4-6` | `--anthropic-key` |
| OpenAI | `openai` | `openai/gpt-5-mini` | `--openai-key` |
| Google Gemini | `gemini` | `google/gemini-2.5-flash` | `--gemini-key` |
| BytePlus ARK | `byteplus` | `byteplus/ark-code-latest` | `--byteplus-ark-api-key` |
| OpenCode (MiniMax M2.5 Free) | `opencode` | `opencode/minimax-m2.5-free` | none (free) |

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

# Switch to OpenCode (MiniMax M2.5 Free, no API key required)
clawmacdo update-model --instance <deploy-id> \
  --primary-model opencode
```

The command updates API keys in `.env`, configures provider settings (BytePlus `openclaw.json`), sets the model via `openclaw models set`, adds failovers, and restarts the gateway service. When `opencode` is selected, OpenCode is installed via `curl -fsSL https://opencode.ai/install | bash` and configured with MiniMax M2.5 Free (`opencode/minimax-m2.5-free`) — no API key required. API keys for other providers are optional — if omitted, the existing key on the instance is preserved.

### Update Bedrock Token for the TTY Proxy

OpenClaw deployments that point to the local Ollama-compatible tty proxy keep the OpenClaw model selection unchanged. To rotate the AWS Bedrock token used by that proxy, update the tty proxy project `.env` under `/home/openclaw/.openclaw/workspace`:

```bash
clawmacdo bedrock-token-set --instance <deploy-id> \
  --bearer-token "$AWS_BEARER_TOKEN_BEDROCK"

# If auto-detection finds the wrong workspace project, pass the .env explicitly.
clawmacdo bedrock-token-set --instance <deploy-id> \
  --bearer-token "$AWS_BEARER_TOKEN_BEDROCK" \
  --env-file claw-tty-proxy/.env
```

The command installs `dotenvx` on the instance if needed and writes `AWS_BEARER_TOKEN_BEDROCK` with dotenvx encryption. It handles an already-encrypted `.env` by using `dotenvx set --encrypt` instead of editing ciphertext with `sed`.

### Set OPENCLAW_INSTANCE for the TTY Proxy

Update the `OPENCLAW_INSTANCE` value in the claw-ttyproxy project `.env` (`/home/openclaw/.openclaw/workspace/claw-ttyproxy/.env`) on a deployed instance:

```bash
clawmacdo ttyproxy-instance-set --instance <deploy-id> \
  --openclaw-instance openclaw-prod-1

# If the tty proxy project lives in a different workspace directory, pass the .env explicitly.
clawmacdo ttyproxy-instance-set --instance <deploy-id> \
  --openclaw-instance openclaw-prod-1 \
  --env-file claw-tty-proxy/.env
```

The command replaces the existing `OPENCLAW_INSTANCE=` line in place (or appends one if missing), leaves a timestamped `.bak` beside the file, and keeps every other key untouched. Restart the tty proxy afterwards so it picks up the new value.

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

### Fast Lightsail Restore + OpenClaw Readiness

Restore a Lightsail snapshot and immediately configure Telegram pairing, OpenClaw identity, and the Remotion avatar `.env` once SSH is ready. The command batches remote edits into one SSH session and checks the Tailscale public URL and Remotion cloudflared tunnel in parallel.

```bash
clawmacdo ls-restore-fast \
  --snapshot-name "my-openclaw-backup" \
  --telegram-bot-token "$TELEGRAM_TOKEN" \
  --owner-name "Kenny" \
  --openclaw-name "John" \
  --avatar-name "John" \
  --json
```

Use `--telegram-pair-code <CODE>` when you already have an 8-character Telegram pairing code to approve in the same run. The default fast budgets are `--active-timeout-secs 120` and `--ssh-timeout-secs 30`; actual restore time still depends on Lightsail making the instance runnable.

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

### Memory Archive Download

```bash
# Download all memory archives from an instance to the current directory
clawmacdo memory-download --instance my-server

# Download to a specific output directory
clawmacdo memory-download --instance my-server --output ~/backups/

# Download to a specific file path
clawmacdo memory-download --instance 1.2.3.4 --output ~/backups/memory.tar.gz
```

The command SSHes into the instance, creates a tar.gz of all files under `/home/openclaw/.openclaw/memory/`, downloads it locally, and cleans up the temporary archive on the remote host.

### OpenClaw Identity

```bash
clawmacdo openclaw-identity --instance my-server \
  --openclaw-name "Clawdia" \
  --owner-name "Kenneth"
```

This command sets the target agent's OpenClaw identity and writes managed owner context into the remote workspace `USER.md`, then restarts the gateway.

### Gyne Consumer Profile

```bash
clawmacdo gyne-consumer-profile --instance my-server --name consumer-4

# Equivalent terminology for the profile name
clawmacdo gyne-consumer-profile --instance my-server --consumer-name consumer-4

# Restart a specific unit, or skip the restart entirely
clawmacdo gyne-consumer-profile --instance my-server --name consumer-4 --service gyne-agent.service
clawmacdo gyne-consumer-profile --instance my-server --name consumer-4 --no-restart

# Non-default workspace project or base task stream
clawmacdo gyne-consumer-profile --instance my-server \
  --project gyne-agent \
  --task-stream openclaw:tasks \
  --name consumer-4 \
  --json
```

This command SSHes into the instance as the OpenClaw user, resolves the configured workspace for `--agent` (default `main`), and updates only `workspace/<project>/.env` (default `gyne-agent/.env`). It sets `CONSUMER_NAME=<name>` and `CONSUMER_TASK_STREAM=<TASK_STREAM>:<name>`, preserving the rest of the `.env` and creating a `.clawmacdo-gyne-*.bak` backup beside it. If `--task-stream` is omitted, the command derives the base stream from the existing `TASK_STREAM` value, falling back to `openclaw:tasks`.

By default it then restarts the Gyne consumer `systemctl --user` service so the worker re-registers under the new `CONSUMER_NAME` (changing `.env` alone does not affect a running process). The unit is auto-detected from the enabled `--user` services (a `gyne`/`consumer` unit, excluding the gateway); pass `--service <unit>` to target a specific unit, or `--no-restart` to only edit `.env`.

### OpenClaw Markdown Download

```bash
clawmacdo openclaw-md-download --instance my-server --output ~/backups/
```

Downloads the active OpenClaw workspace Markdown context files as a ZIP: `AGENTS.md`, `SOUL.md`, `IDENTITY.md`, `USER.md`, `TOOLS.md`, `HEARTBEAT.md`, `BOOTSTRAP.md`, `llm_wiki.md` when present, plus daily memory logs under `memory/*.md`.

### OpenClaw LLM Wiki

```bash
clawmacdo openclaw-llm-wiki --instance my-server \
  --project remotion-avatar \
  --title "Project LLM Wiki" \
  --prompt "Include architecture, runbooks, and open questions."

# Upload an existing Markdown file from a web backend or local path
clawmacdo openclaw-llm-wiki --instance my-server \
  --project remotion-avatar \
  --llm-wiki-md ./llm_wiki.md \
  --skip-claude \
  --json
```

SSHes into the instance as the OpenClaw user, resolves the target agent workspace, creates or uploads an attachable `llm_wiki.md` under `workspace/<project>/`, seeds that project wiki folder, then optionally launches Claude Code inside the project directory to refine the wiki structure. `--project` defaults to `llm_wiki`; `--llm-wiki-md` uploads any local Markdown file as `<project>/llm_wiki.md`; `--skip-claude` makes the command upload/seed only. `--json` returns `{ ok, project, files, claude_status, error }` plus paths so web apps can display Claude failure details.

### Claude Code Reconnect

```bash
clawmacdo claude-auth-start --instance my-server --json
clawmacdo claude-auth-status --instance my-server --json
```

`claude-auth-start` SSHes into the instance as the OpenClaw user, starts Claude Code auth in a remote PTY, captures the generated login URL, and returns JSON for app integrations:

```json
{
  "ok": true,
  "valid": false,
  "status": "pending",
  "login_url": "https://claude.ai/...",
  "instance": "my-server"
}
```

After the user completes the browser flow, poll `claude-auth-status --json` until `valid` is `true` and `status` is `authenticated`. The status command exits successfully while auth is still pending, so web apps can parse the JSON instead of scraping SSH errors.

### OpenClaw Wiki Files

```bash
clawmacdo wiki-tree --instance my-server --project llm_wiki --json
clawmacdo wiki-index --instance my-server --project llm_wiki --json
clawmacdo wiki-read --instance my-server --path llm_wiki/INDEX.md --json
clawmacdo wiki-write --instance my-server --path llm_wiki/INDEX.md \
  --content-file /tmp/INDEX.md \
  --base-sha <sha-from-wiki-read> \
  --json
clawmacdo wiki-write --instance my-server --path llm_wiki/new-page.md \
  --content-file /tmp/new-page.md \
  --base-sha NEW \
  --json
clawmacdo wiki-ingest --instance my-server --project wiki-163327 \
  --source /tmp/uploaded-doc.md \
  --prompt "Decide where this belongs, update existing pages if needed, create new pages if needed, update index/log." \
  --json
clawmacdo wiki-export --instance my-server --project llm_wiki --output ~/backups/ --json
clawmacdo wiki-delete --instance my-server --project wiki-163327 --json
```

These commands resolve the configured OpenClaw agent workspace on the instance and only operate on safe relative Markdown paths under that workspace. `wiki-read` returns `content`, `sha256`, `mtime`, and `size`; `wiki-write` requires `--base-sha` so a web app cannot overwrite a file that changed after it was opened. Use `--base-sha NEW` only when creating a new file. `wiki-index` returns per-page hashes plus headings, tags, and Markdown/wiki links for graph or navigation UIs. `wiki-ingest` uploads a converted Markdown source into `<project>/raw/sources/`, launches Claude Code inside that project to decide where the content belongs, and returns `{ project, source_files, changed_files, summary }` JSON for app integrations. `wiki-delete` is intentionally narrower: it only deletes a direct `workspace/wiki-*` project folder and rejects empty names, absolute paths, traversal, symlinks, files, and non-`wiki-*` slugs.

### Gateway Token Rotation

```bash
clawmacdo openclaw-gateway-token --instance my-server
clawmacdo openclaw-gateway-url --instance my-server --json
```

Regenerates `gateway.auth.token` in `/home/openclaw/.openclaw/openclaw.json`, keeps password auth in sync when configured, backs up the old config to `openclaw.json.bak`, and restarts the gateway.

`openclaw-gateway-url --json` returns the current public Tailscale Funnel Gateway URL when available:

```json
{
  "ok": true,
  "public_url": "https://example.ts.net",
  "gateway_url": "https://example.ts.net/auth.html#...",
  "gateway_token_present": true
}
```

### Remotion Avatar Setup

```bash
clawmacdo remotion-avatar-setup --instance my-server --name "Kenny" \
  --openai-api-key "$OPENAI_API_KEY" \
  --voice-gender female \
  --avatar-glb ./kenny_avatar.glb \
  --json
```

Configures `/home/openclaw/.openclaw/workspace/remotion-3d-AI-avatar/.env` with `CHAT_BASE_URL=http://127.0.0.1:18789/v1`, `CHAT_API_KEY` from the OpenClaw gateway token, `CHAT_MODEL=openclaw`, `VITE_AVATAR_NAME` from `--name`, `OPENAI_API_KEY` from `--openai-api-key`/`OPENAI_API_KEY`, and voice settings (`VOICE_GENDER`, `TTS_VOICE`; male maps to `onyx`, female maps to `nova`); optionally uploads `avatar.glb` or `<userid>_avatar.glb` to the app as `public/avatar.glb`; replaces `kenken64` with the provided name; starts the app; starts a free Cloudflare Quick Tunnel to the frontend port; and prints the public `trycloudflare.com` URL.

With `--json`, the command returns a stable object for web apps:

```json
{
  "ok": true,
  "remotion_url": "https://example.trycloudflare.com",
  "remotion_service": "active",
  "tunnel_service": "active"
}
```

### Scheduled Cron Jobs

```bash
# Add a scheduled message (sent via telegram on a cron schedule)
clawmacdo cron-message \
  --instance my-server \
  --name "daily-digest" \
  --schedule "0 9 * * *" \
  --message "Summarise today's headlines" \
  --channel telegram

# Add a scheduled message using an interval instead of a cron expression
clawmacdo cron-message \
  --instance my-server \
  --name "hourly-check" \
  --every 1h \
  --message "What is the current BTC price?" \
  --channel telegram \
  --to 7547736315

# Add a scheduled tool execution
clawmacdo cron-tool \
  --instance my-server \
  --name "weekly-report" \
  --schedule "0 8 * * 1" \
  --tool web-search \
  --args "latest AI news" \
  --channel telegram

# List all cron jobs on an instance
clawmacdo cron-list --instance my-server

# Remove a cron job by name
clawmacdo cron-remove --instance my-server --name daily-digest
```

> **Performance note:** All cron commands batch the device-pairing approval step with the first gateway command into a single SSH session, minimising handshake overhead.

### Webhook Hooks

```bash
# Enable webhook hooks on an instance (generates token, creates default notify mapping)
clawmacdo hooks-enable --instance my-server

# Send a task to the agent via webhook (delivers response to Telegram)
clawmacdo hooks-send --instance my-server --task "Check today's weather for Singapore"

# Send to a specific mapping
clawmacdo hooks-send --instance my-server --task "Generate a report" --mapping notify

# Show hooks status and all mappings
clawmacdo hooks-status --instance my-server

# Disable webhook hooks
clawmacdo hooks-disable --instance my-server
```

External services can also POST directly:
```bash
curl -X POST https://<funnel-url>/hooks/notify \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"task":"your instruction to the agent"}'
```

> **Template note:** The `messageTemplate` uses `{{task}}` to extract the `task` field from the JSON payload. Other available template variables: `{{payload.field}}`, `{{headers.X-Header}}`, `{{query.param}}`, `{{path}}`, `{{now}}`.

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

// --- Check WhatsApp status ---
const status = execSync(`${bin} whatsapp-status --instance <deploy-id>`, { encoding: "utf8" });
console.log(status); // "connected", "pending_qr", etc.

// --- Wait for WhatsApp scan (blocks until connected or timeout) ---
execSync(`${bin} whatsapp-wait --instance <deploy-id> --timeout 120`, { stdio: "inherit" });

// --- Change AI model ---
execSync(`${bin} update-model \
  --instance <deploy-id> \
  --primary-model openai \
  --openai-key "${process.env.OPENAI_API_KEY}"`, { stdio: "inherit" });

// --- Rotate Bedrock token for the OpenClaw workspace tty proxy ---
execSync(`${bin} bedrock-token-set \
  --instance <deploy-id> \
  --bearer-token "${process.env.AWS_BEARER_TOKEN_BEDROCK}"`, { stdio: "inherit" });

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
| `CLAWMACDO_STATE_DIR` | Directory for deploy records, SSH keys, backups, and `deployments.db` (default: `~/.clawmacdo`) | Optional |
| `CLAWMACDO_TOKIO_WORKER_THREADS` | Tokio async runtime worker threads (default capped at `2`) | Optional |
| `CLAWMACDO_MAX_CONCURRENCY` | Caps Tokio blocking threads for SSH/SCP-heavy work (default `8`) | Optional |
| `CLAWMACDO_RAYON_NUM_THREADS` | Mirrors to `RAYON_NUM_THREADS` before runtime startup | Optional |
| `CLAWMACDO_SPAWN_RETRIES` | Retries detached deploy child process spawn failures | Optional |
| `CLAWMACDO_SPAWN_RETRY_DELAY_MS` | Delay between detached deploy spawn retries in milliseconds | Optional |
| `SKILLS_API_URL` | Railway skills API base URL | For skill commands |
| `USER_SKILLS_API_KEY` | API key for user-skills endpoints | For skill commands |

## Railway Persistence

When running `clawmacdo serve` on Railway, attach a Railway Volume at:

```text
/app/.clawmacdo
```

This repository includes a root `Dockerfile` and `railway.toml` for the web UI. Railway should build the Rust binary and start:

```sh
clawmacdo serve --port "$PORT"
```

If Railway still starts `node /app/server.js`, remove the service-level start command override in Railway and redeploy from this repository config.

Set `CLAWMACDO_STATE_DIR=/app/.clawmacdo` so deploy records, SSH keys, backups, and `deployments.db` survive redeploys. The Docker image sets this by default. If Railway provides `RAILWAY_VOLUME_MOUNT_PATH=/app/.clawmacdo`, clawmacdo also auto-detects that mount path.

Railway Volumes are mounted as root by default. For non-root container images, either set `RAILWAY_RUN_UID=0` as a quick operational fix, or chown the mounted directory in the hosting app image before starting the server:

```sh
mkdir -p /app/.clawmacdo
chown -R nextjs:nodejs /app/.clawmacdo
exec "$@"
```

Without a persistent volume, Railway redeploys can remove `deploys/`, `keys/`, and `deployments.db`. Existing cloud instances may then be unreachable from clawmacdo unless the old deploy record and SSH private key can be recovered; after attaching the volume, destroy/recreate those orphaned instances and provision again.

Railway references: [Volumes](https://docs.railway.com/volumes), [Volume permissions](https://docs.railway.com/reference/volumes).

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

# Run the full Rust quality pipeline
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# Build specific crate
cargo build -p clawmacdo-core

# Update dependencies
cargo update
```

The workspace now includes focused Rust tests around config/path validation, deployment database lookups, cron/hook command construction helpers, QR parsing, and web middleware behavior. Add new tests close to the module they protect.

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
- Deploy Step 15 gateway health check uses exponential backoff (max ~70s) instead of 150 fixed 1s polls; model setup and profile setup are batched into a single SSH session per provider.
- Direct Docker-group access for `openclaw` has been removed. If sandbox mode is requested during deploy, the deploy now forces sandbox mode off until a safer non-root mediation path exists.
- Lightsail credentials are passed only to the child AWS CLI processes instead of mutating process-global environment variables or writing `~/.aws/credentials`.
- Tencent's optional security-group helper now takes SSH ingress from `CLAWMACDO_TENCENT_SSH_CIDR` and defaults to `127.0.0.1/32` instead of opening SSH to the world.

See [docs/HIGH_SECURITY_FIXES.md](docs/HIGH_SECURITY_FIXES.md) for the finding-by-finding code map, rationale, and functionality impact.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Run `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test`
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
| [Hermes Native Bundles](scripts/hermes-bundle/README.md) | Pinned runtime versions, clean builds, verification, and bundle publishing |
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

**Current version:** 0.98.1



