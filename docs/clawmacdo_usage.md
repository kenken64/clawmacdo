# ClawMacdo CLI Usage Guide

Complete reference for all `clawmacdo` subcommands with examples, equivalent curl commands, and sample responses.

> **Note:** All credentials in examples are placeholders. Replace `<YOUR_*>` values with your actual keys.

---

## Table of Contents

- [deploy](#deploy) — Deploy a new OpenClaw instance
- [track](#track) — Track deployment progress
- [destroy](#destroy) — Destroy a deployed instance
- [telegram-setup](#telegram-setup) — Configure Telegram bot on an instance
- [telegram-pair](#telegram-pair) — Approve Telegram pairing code
- [tailscale-funnel](#tailscale-funnel) — Set up Tailscale Funnel for public HTTPS access
- [funnel-on](#funnel-on) — Enable Tailscale Funnel on an instance
- [funnel-off](#funnel-off) — Disable Tailscale Funnel on an instance
- [device-approve](#device-approve) — Approve pending webchat device pairing requests
- [skill-upload](#skill-upload) — Upload a SKILL.md to the skills API and instance
- [skill-download](#skill-download) — Download a SKILL.md from the skills API
- [skill-push](#skill-push) — Push a SKILL.md from the skills API to the instance
- [ark-api-key](#ark-api-key) — Generate BytePlus ARK API key or list endpoints
- [ark-chat](#ark-chat) — Send a prompt to a BytePlus ARK model
- [serve](#serve) — Start the web UI server
- [Environment Variables](#environment-variables)
- [Web UI API Endpoints](#web-ui-api-endpoints)

---

## deploy

Deploy a new OpenClaw instance to any supported cloud provider.

### Syntax

```
clawmacdo deploy --provider <PROVIDER> --customer-email <EMAIL> [OPTIONS]
```

### Provider Aliases

| Provider | `--provider` value | Alias |
|----------|-------------------|-------|
| DigitalOcean | `digitalocean` | `do` |
| Tencent Cloud | `tencent` | `tc` |
| AWS Lightsail | `lightsail` | `aws` |
| Microsoft Azure | `azure` | `az` |
| BytePlus Cloud | `byteplus` | `bp` |

---

### DigitalOcean

```bash
# Using environment variable
export DO_TOKEN="<YOUR_DO_TOKEN>"

clawmacdo deploy \
  --provider digitalocean \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

```bash
# Using CLI flag
clawmacdo deploy \
  --provider do \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --do-token "<YOUR_DO_TOKEN>" \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>" \
  --region sgp1 \
  --size s-2vcpu-4gb
```

**Default region:** `sgp1` | **Default size:** `s-2vcpu-4gb`

#### DigitalOcean Instance Sizes

| `--size` | vCPU | RAM | Price |
|----------|------|-----|-------|
| `s-1vcpu-2gb` | 1 | 2 GB | ~$12/mo |
| `s-2vcpu-4gb` *(default)* | 2 | 4 GB | ~$24/mo |
| `s-4vcpu-8gb` | 4 | 8 GB | ~$48/mo |

---

### Tencent Cloud

```bash
export TENCENT_SECRET_ID="<YOUR_SECRET_ID>"
export TENCENT_SECRET_KEY="<YOUR_SECRET_KEY>"

clawmacdo deploy \
  --provider tencent \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

```bash
# Hong Kong region with custom size
clawmacdo deploy \
  --provider tc \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --tencent-secret-id "<YOUR_SECRET_ID>" \
  --tencent-secret-key "<YOUR_SECRET_KEY>" \
  --region ap-hongkong \
  --size SA5.MEDIUM8 \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

**Default region:** `ap-singapore` | **Default size:** `SA5.MEDIUM4`

---

### AWS Lightsail

> **Prerequisite:** [AWS CLI](https://aws.amazon.com/cli/) must be installed. ClawMacdo will attempt to auto-install it if missing.

```bash
export AWS_ACCESS_KEY_ID="<YOUR_ACCESS_KEY>"
export AWS_SECRET_ACCESS_KEY="<YOUR_SECRET_KEY>"

clawmacdo deploy \
  --provider lightsail \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --aws-region ap-southeast-1 \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

```bash
# US East region with larger instance
clawmacdo deploy \
  --provider aws \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --aws-access-key-id "<YOUR_ACCESS_KEY>" \
  --aws-secret-access-key "<YOUR_SECRET_KEY>" \
  --aws-region us-east-1 \
  --size s-4vcpu-8gb \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

**Default region:** `ap-southeast-1` | **Default size:** `s-2vcpu-4gb`

#### Lightsail Instance Sizes

| `--size` | Lightsail Bundle | vCPU | RAM | Price |
|----------|-----------------|------|-----|-------|
| `s-1vcpu-2gb` | `small_3_0` | 1 | 2 GB | ~$10/mo |
| `s-2vcpu-4gb` *(default)* | `medium_3_0` | 2 | 4 GB | ~$20/mo |
| `s-4vcpu-8gb` | `large_3_0` | 4 | 8 GB | ~$40/mo |

---

### Microsoft Azure

> **Prerequisite:** [Azure CLI](https://learn.microsoft.com/en-us/cli/azure/) must be installed. ClawMacdo will attempt to auto-install it if missing.

```bash
export AZURE_TENANT_ID="<YOUR_TENANT_ID>"
export AZURE_SUBSCRIPTION_ID="<YOUR_SUBSCRIPTION_ID>"
export AZURE_CLIENT_ID="<YOUR_CLIENT_ID>"
export AZURE_CLIENT_SECRET="<YOUR_CLIENT_SECRET>"

clawmacdo deploy \
  --provider azure \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

```bash
# Explicit flags with larger VM
clawmacdo deploy \
  --provider az \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --azure-tenant-id "<YOUR_TENANT_ID>" \
  --azure-subscription-id "<YOUR_SUBSCRIPTION_ID>" \
  --azure-client-id "<YOUR_CLIENT_ID>" \
  --azure-client-secret "<YOUR_CLIENT_SECRET>" \
  --region southeastasia \
  --size Standard_B2ms \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

**Default region:** `southeastasia` | **Default size:** `Standard_B2s`

---

### BytePlus Cloud

```bash
export BYTEPLUS_ACCESS_KEY="<YOUR_ACCESS_KEY>"
export BYTEPLUS_SECRET_KEY="<YOUR_SECRET_KEY>"

clawmacdo deploy \
  --provider byteplus \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --byteplus-ark-api-key "<YOUR_ARK_API_KEY>" \
  --primary-model byteplus \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

```bash
# With custom size
clawmacdo deploy \
  --provider bp \
  --customer-name "my-instance" \
  --customer-email "user@example.com" \
  --byteplus-access-key "<YOUR_ACCESS_KEY>" \
  --byteplus-secret-key "<YOUR_SECRET_KEY>" \
  --region ap-southeast-1 \
  --size ecs.g3i.xlarge \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

**Default region:** `ap-southeast-1` | **Default size:** `ecs.g3i.large`

#### BytePlus Instance Sizes

| `--size` | vCPU | RAM | Type |
|----------|------|-----|------|
| `ecs.c3i.large` | 2 | 4 GB | Compute-optimized |
| `ecs.g3i.large` *(default)* | 2 | 8 GB | General purpose |
| `ecs.c3i.xlarge` | 4 | 8 GB | Compute-optimized |
| `ecs.g3i.xlarge` | 4 | 16 GB | General purpose |

---

### Deploy Options (All Providers)

#### AI Model Configuration

```bash
# Single model (Anthropic as primary)
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --primary-model anthropic \
  --anthropic-key "<YOUR_KEY>"

# Multiple models with failover chain
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --primary-model anthropic \
  --failover-1 openai \
  --failover-2 gemini \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>" \
  --openai-key "<YOUR_OPENAI_KEY>" \
  --gemini-key "<YOUR_GEMINI_KEY>"

# BytePlus ARK as primary with Anthropic failover
clawmacdo deploy \
  --provider bp \
  --customer-email "user@example.com" \
  --primary-model byteplus \
  --failover-1 anthropic \
  --byteplus-access-key "<YOUR_ACCESS_KEY>" \
  --byteplus-secret-key "<YOUR_SECRET_KEY>" \
  --byteplus-ark-api-key "<YOUR_ARK_API_KEY>" \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>"
```

**Supported models:** `anthropic`, `openai`, `gemini`, `byteplus`

#### Messaging Channels

```bash
# With Telegram bot
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --telegram-bot-token "<YOUR_BOT_TOKEN>" \
  --anthropic-key "<YOUR_KEY>"

# With WhatsApp
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --whatsapp-phone-number "+1234567890" \
  --anthropic-key "<YOUR_KEY>"
```

#### Tailscale VPN

```bash
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --tailscale \
  --tailscale-auth-key "<YOUR_TAILSCALE_KEY>" \
  --anthropic-key "<YOUR_KEY>"
```

#### Backup Restore

```bash
# Deploy with backup restore
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --backup ~/backups/openclaw-2026-03-15.tar.gz \
  --anthropic-key "<YOUR_KEY>"
```

#### Custom Hostname

```bash
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --hostname "my-custom-host.openclaw.dev" \
  --anthropic-key "<YOUR_KEY>"
```

#### Profiles

```bash
# Full profile (default — all features)
clawmacdo deploy --provider do --customer-email "user@example.com" --profile full

# Messaging-only profile
clawmacdo deploy --provider do --customer-email "user@example.com" --profile messaging

# Coding-only profile
clawmacdo deploy --provider do --customer-email "user@example.com" --profile coding
```

#### Sandbox Mode

```bash
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --enable-sandbox \
  --anthropic-key "<YOUR_KEY>"
```

#### Provider Backups

```bash
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --enable-backups \
  --anthropic-key "<YOUR_KEY>"
```

#### Detach Mode (Background Deploy)

```bash
# Start deploy in background, returns immediately with deploy ID
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --anthropic-key "<YOUR_KEY>" \
  --detach
```

Sample output:
```
Deploy ID: a1b2c3d4-e5f6-7890-abcd-ef1234567890
Tracking: clawmacdo track a1b2c3d4-e5f6-7890-abcd-ef1234567890 --follow
Log file: /Users/you/.clawmacdo/deploy-a1b2c3d4.log
```

#### JSON Output

```bash
clawmacdo deploy \
  --provider do \
  --customer-email "user@example.com" \
  --anthropic-key "<YOUR_KEY>" \
  --json
```

#### Full Deploy (All Options Combined)

```bash
clawmacdo deploy \
  --provider digitalocean \
  --customer-name "production-openclaw" \
  --customer-email "admin@company.com" \
  --do-token "<YOUR_DO_TOKEN>" \
  --region sgp1 \
  --size s-4vcpu-8gb \
  --hostname "prod.openclaw.dev" \
  --primary-model anthropic \
  --failover-1 openai \
  --failover-2 gemini \
  --anthropic-key "<YOUR_ANTHROPIC_KEY>" \
  --openai-key "<YOUR_OPENAI_KEY>" \
  --gemini-key "<YOUR_GEMINI_KEY>" \
  --telegram-bot-token "<YOUR_BOT_TOKEN>" \
  --whatsapp-phone-number "+1234567890" \
  --tailscale \
  --tailscale-auth-key "<YOUR_TAILSCALE_KEY>" \
  --backup ~/backups/openclaw-backup.tar.gz \
  --enable-backups \
  --enable-sandbox \
  --profile full \
  --detach \
  --json
```

---

## track

Track a deployment's progress by deploy ID, hostname, or IP address.

### Syntax

```
clawmacdo track <QUERY> [--follow] [--json]
```

### Examples

```bash
# Track by deploy ID
clawmacdo track a1b2c3d4-e5f6-7890-abcd-ef1234567890

# Track by hostname
clawmacdo track my-instance.openclaw.dev

# Track by IP address
clawmacdo track 128.199.123.45

# Follow mode — live refresh until complete
clawmacdo track a1b2c3d4-e5f6-7890-abcd-ef1234567890 --follow

# JSON output (NDJSON format)
clawmacdo track a1b2c3d4-e5f6-7890-abcd-ef1234567890 --json

# Follow + JSON (streaming NDJSON)
clawmacdo track a1b2c3d4-e5f6-7890-abcd-ef1234567890 --follow --json
```

### Sample Output (Human-Readable)

```
Deploy: a1b2c3d4-e5f6-7890-abcd-ef1234567890
Status: running
Provider: digitalocean
Hostname: my-instance.openclaw.dev
IP: 128.199.123.45
Started: 2026-03-15T10:30:00Z

Steps:
  [1/16] Generate SSH key pair              ✅ completed
  [2/16] Upload SSH key to cloud            ✅ completed
  [3/16] Create cloud instance              ✅ completed
  [4/16] Wait for instance to be active     ✅ completed
  [5/16] Wait for SSH connectivity          ✅ completed
  [6/16] Wait for cloud-init                ✅ completed
  [7/16] Upload backup archive              ⏭️  skipped
  [8/16] Create openclaw user               🔄 running
  [9/16] Install Node.js                    ⏳ pending
  ...
```

### Sample Output (JSON)

```json
{
  "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "status": "running",
  "provider": "digitalocean",
  "hostname": "my-instance.openclaw.dev",
  "ip_address": "128.199.123.45",
  "created_at": "2026-03-15T10:30:00Z",
  "steps": [
    {
      "step_number": 1,
      "total_steps": 16,
      "label": "Generate SSH key pair",
      "status": "completed",
      "started_at": "2026-03-15T10:30:01Z",
      "completed_at": "2026-03-15T10:30:02Z"
    }
  ]
}
```

---

## destroy

Destroy a deployed cloud instance and clean up local records.

### Syntax

```
clawmacdo destroy --provider <PROVIDER> [--name <NAME>] [--yes] [CREDENTIALS...]
```

### Examples

```bash
# Destroy a DigitalOcean instance (interactive selection)
clawmacdo destroy --provider digitalocean --do-token "<YOUR_DO_TOKEN>"

# Destroy a specific instance by name
clawmacdo destroy --provider do --name "my-instance" --do-token "<YOUR_DO_TOKEN>"

# Skip confirmation prompt
clawmacdo destroy --provider do --name "my-instance" --do-token "<YOUR_DO_TOKEN>" --yes

# Destroy Tencent Cloud instance
clawmacdo destroy \
  --provider tencent \
  --name "my-instance" \
  --tencent-secret-id "<YOUR_SECRET_ID>" \
  --tencent-secret-key "<YOUR_SECRET_KEY>"

# Destroy AWS Lightsail instance
clawmacdo destroy \
  --provider lightsail \
  --name "my-instance" \
  --aws-region ap-southeast-1

# Destroy Azure instance
clawmacdo destroy \
  --provider azure \
  --name "my-instance" \
  --azure-tenant-id "<YOUR_TENANT_ID>" \
  --azure-subscription-id "<YOUR_SUBSCRIPTION_ID>" \
  --azure-client-id "<YOUR_CLIENT_ID>" \
  --azure-client-secret "<YOUR_CLIENT_SECRET>" \
  --azure-resource-group "my-resource-group"

# Destroy BytePlus instance
clawmacdo destroy \
  --provider byteplus \
  --name "my-instance" \
  --byteplus-access-key "<YOUR_ACCESS_KEY>" \
  --byteplus-secret-key "<YOUR_SECRET_KEY>"

# Using environment variables
export BYTEPLUS_ACCESS_KEY="<YOUR_ACCESS_KEY>"
export BYTEPLUS_SECRET_KEY="<YOUR_SECRET_KEY>"
clawmacdo destroy --provider bp --name "my-instance" --yes
```

### Sample Output

```
Found instance: my-instance (128.199.123.45)
⚠️  This will permanently destroy the instance and all its data.
Proceed? [y/N]: y
Destroying instance my-instance...
Instance destroyed.
Local deploy record removed.
SSH key cleaned up.
```

---

## telegram-setup

Configure a Telegram bot token on a deployed OpenClaw instance. SSHs into the instance, sets the bot token in `.env`, enables the Telegram plugin, restarts the gateway, and triggers the pairing flow.

### Syntax

```
clawmacdo telegram-setup --instance <QUERY> --bot-token <TOKEN>
```

### Examples

```bash
# By deploy ID
clawmacdo telegram-setup \
  --instance a1b2c3d4-e5f6-7890-abcd-ef1234567890 \
  --bot-token "<YOUR_TELEGRAM_BOT_TOKEN>"

# By hostname
clawmacdo telegram-setup \
  --instance my-instance.openclaw.dev \
  --bot-token "<YOUR_TELEGRAM_BOT_TOKEN>"

# By IP address
clawmacdo telegram-setup \
  --instance 128.199.123.45 \
  --bot-token "<YOUR_TELEGRAM_BOT_TOKEN>"
```

### Sample Output

```
Configuring Telegram bot on 128.199.123.45...
[1/4] Setting TELEGRAM_BOT_TOKEN in .env...
[2/4] Enabling Telegram plugin...
  Plugin telegram enabled
[3/4] Restarting gateway service...
  gateway: active
[4/4] Starting Telegram channel login...

Send /start to your bot to receive a pairing code.
Then run: clawmacdo telegram-pair --instance my-instance.openclaw.dev --code <PAIRING_CODE>
```

---

## telegram-pair

Approve a Telegram pairing code to activate chat on an OpenClaw instance.

### Syntax

```
clawmacdo telegram-pair --instance <QUERY> --code <CODE>
```

The pairing code is an 8-character alphanumeric string obtained by sending `/start` to your Telegram bot after running `telegram-setup`.

### Examples

```bash
# By deploy ID
clawmacdo telegram-pair \
  --instance a1b2c3d4-e5f6-7890-abcd-ef1234567890 \
  --code ABCD1234

# By hostname
clawmacdo telegram-pair \
  --instance my-instance.openclaw.dev \
  --code VGUB4Z6K

# By IP address
clawmacdo telegram-pair \
  --instance 128.199.123.45 \
  --code XY9Z8W7V
```

### Sample Output

```
Approving Telegram pairing code VGUB4Z6K on 128.199.123.45...
Pairing approved for user 123456789
Telegram pairing approved. Send a message to your bot to start chatting.
```

---

## tailscale-funnel

Set up Tailscale Funnel on a deployed OpenClaw instance for public HTTPS access. Performs a full 6-step setup: install Tailscale, connect with auth key, enable Funnel, retrieve public URL, configure `openclaw.json` (`controlUi.allowedOrigins` + `trustedProxies`), and auto-approve pending devices.

### Syntax

```
clawmacdo tailscale-funnel --instance <QUERY> --auth-key <TAILSCALE_AUTH_KEY> [--port <PORT>]
```

### Examples

```bash
# By deploy ID
clawmacdo tailscale-funnel \
  --instance a1b2c3d4-e5f6-7890-abcd-ef1234567890 \
  --auth-key "tskey-auth-abc123..."

# By hostname with custom port
clawmacdo tailscale-funnel \
  --instance my-instance.openclaw.dev \
  --auth-key "tskey-auth-abc123..." \
  --port 8080

# By IP address (using env var)
export TAILSCALE_AUTH_KEY="tskey-auth-abc123..."
clawmacdo tailscale-funnel \
  --instance 128.199.123.45
```

### Sample Output

```
Setting up Tailscale Funnel on 128.199.123.45...

[1/6] Installing Tailscale...
  Tailscale installed successfully.
[2/6] Connecting Tailscale...
  Tailscale connected.
[3/6] Enabling Tailscale Funnel on port 18789...
  Available on the internet:
[4/6] Retrieving Funnel public URL...
  https://openclaw-ff54485d.tail12345.ts.net:
  |-- / proxy http://127.0.0.1:18789

  Public URL: https://openclaw-ff54485d.tail12345.ts.net
[5/6] Updating openclaw.json (allowedOrigins + trustedProxies)...
  allowedOrigins: ["https://openclaw-ff54485d.tail12345.ts.net"]
  trustedProxies: ["127.0.0.1/8","::1/128"]

Restarting OpenClaw gateway...
  gateway: active
[6/6] Approving all pending devices...
  No pending devices found.

Tailscale Funnel setup complete!
Public URL: https://openclaw-ff54485d.tail12345.ts.net
Webchat:    https://openclaw-ff54485d.tail12345.ts.net/chat?token=<auth-token>

Note: If you connect from a new browser, approve it with:
  clawmacdo device-approve --instance 128.199.123.45
```

---

## funnel-on

Enable Tailscale Funnel on a deployed instance. Requires Tailscale to be already installed and connected (see `tailscale-funnel`).

### Syntax

```
clawmacdo funnel-on --instance <QUERY> [--port <PORT>]
```

### Examples

```bash
# Default port (18789)
clawmacdo funnel-on --instance my-instance.openclaw.dev

# Custom port
clawmacdo funnel-on --instance 128.199.123.45 --port 8080
```

### Sample Output

```
Enabling Tailscale Funnel on 128.199.123.45 (port 18789)...

  Available on the internet:
https://openclaw-ff54485d.tail12345.ts.net:
|-- / proxy http://127.0.0.1:18789

Funnel enabled.
```

---

## funnel-off

Disable Tailscale Funnel on a deployed instance.

### Syntax

```
clawmacdo funnel-off --instance <QUERY>
```

### Examples

```bash
clawmacdo funnel-off --instance my-instance.openclaw.dev
clawmacdo funnel-off --instance 128.199.123.45
clawmacdo funnel-off --instance a1b2c3d4-e5f6-7890-abcd-ef1234567890
```

### Sample Output

```
Disabling Tailscale Funnel on 128.199.123.45...

Funnel disabled.
```

---

## device-approve

Approve all pending OpenClaw webchat device pairing requests on a deployed instance. Useful when new browsers connect to the webchat via Tailscale Funnel and need device approval.

### Syntax

```
clawmacdo device-approve --instance <QUERY>
```

### Examples

```bash
clawmacdo device-approve --instance my-instance.openclaw.dev
clawmacdo device-approve --instance 128.199.123.45
```

### Sample Output

```
Approving all pending devices on 128.199.123.45...

  Approved device abc12345-6789-4def-abcd-ef1234567890
  Approved device def67890-1234-4abc-5678-901234567890

Approved 2 device(s).
```

---

## skill-upload

Upload a local SKILL.md to the Railway skills API and deploy it to the OpenClaw instance via SCP. Backs up the existing SKILL.md on both the API server and the instance before overwriting.

### Syntax

```
clawmacdo skill-upload --instance <QUERY> --file <PATH> --api-url <URL> --api-key <KEY>
```

### Examples

```bash
# Using CLI flags
clawmacdo skill-upload \
  --instance my-instance.openclaw.dev \
  --file ./SKILL.md \
  --api-url "https://skills-api.example.com" \
  --api-key "sk-my-secret-key"

# Using environment variables
export SKILLS_API_URL="https://skills-api.example.com"
export USER_SKILLS_API_KEY="sk-my-secret-key"

clawmacdo skill-upload \
  --instance a1b2c3d4-e5f6-7890-abcd-ef1234567890 \
  --file ~/skills/SKILL.md
```

### Sample Output

```
Uploading SKILL.md (2048 bytes) for deployment a1b2c3d4...

[1/3] Uploading to skills API...
  Uploaded to Railway volume.
  Previous version backed up on server.
[2/3] Backing up existing SKILL.md on instance 128.199.123.45...
  Existing SKILL.md backed up.
[3/3] Uploading SKILL.md to instance via SCP...
  SKILL.md deployed to instance.

Done! SKILL.md uploaded to both Railway and instance 128.199.123.45.
```

---

## skill-download

Download a customer SKILL.md from the Railway skills API to a local file.

### Syntax

```
clawmacdo skill-download --instance <QUERY> [--output <PATH>] --api-url <URL> --api-key <KEY>
```

### Examples

```bash
# Download to default path (./SKILL.md)
clawmacdo skill-download \
  --instance my-instance.openclaw.dev \
  --api-url "https://skills-api.example.com" \
  --api-key "sk-my-secret-key"

# Download to a custom path
export SKILLS_API_URL="https://skills-api.example.com"
export USER_SKILLS_API_KEY="sk-my-secret-key"

clawmacdo skill-download \
  --instance a1b2c3d4-e5f6-7890-abcd-ef1234567890 \
  --output ~/skills/downloaded-SKILL.md
```

### Sample Output

```
Downloading SKILL.md for deployment a1b2c3d4...

Downloaded SKILL.md (2048 bytes) to ./SKILL.md
```

---

## skill-push

Push an existing SKILL.md from the Railway skills API directly to the OpenClaw instance. Downloads from the API and SCPs it to the instance, backing up the existing file.

### Syntax

```
clawmacdo skill-push --instance <QUERY> --api-url <URL> --api-key <KEY>
```

### Examples

```bash
# Using CLI flags
clawmacdo skill-push \
  --instance my-instance.openclaw.dev \
  --api-url "https://skills-api.example.com" \
  --api-key "sk-my-secret-key"

# Using environment variables
export SKILLS_API_URL="https://skills-api.example.com"
export USER_SKILLS_API_KEY="sk-my-secret-key"

clawmacdo skill-push --instance 128.199.123.45
```

### Sample Output

```
Pushing SKILL.md from Railway to instance 128.199.123.45...

[1/3] Downloading from skills API...
  Downloaded 2048 bytes.
[2/3] Backing up existing SKILL.md on instance...
  Existing SKILL.md backed up.
[3/3] Uploading to instance via SCP...
  SKILL.md deployed to instance.

Done! SKILL.md pushed to instance 128.199.123.45.
```

---

## ark-api-key

Generate a temporary BytePlus ARK API key from access/secret key credentials, or list available endpoints.

### Syntax

```
clawmacdo ark-api-key --access-key <KEY> --secret-key <KEY> [OPTIONS]
```

### List Endpoints

```bash
# Using environment variables
export BYTEPLUS_ACCESS_KEY="<YOUR_ACCESS_KEY>"
export BYTEPLUS_SECRET_KEY="<YOUR_SECRET_KEY>"
clawmacdo ark-api-key --list

# Using CLI flags
clawmacdo ark-api-key \
  --access-key "<YOUR_ACCESS_KEY>" \
  --secret-key "<YOUR_SECRET_KEY>" \
  --list
```

**Sample Output:**

```
Fetching ARK endpoints...

ENDPOINT ID                    NAME                 STATUS     MODEL
------------------------------------------------------------------------------------------
ep-20260315233753-58rpv        my-endpoint          Running    doubao-1.5-pro-32k
ep-20260310120000-abc12        test-endpoint        Stopped    doubao-1.5-lite-32k

Use --resource-ids <ENDPOINT_ID> to generate an API key for a specific endpoint.
```

### Generate API Key

```bash
# Single endpoint, default 7-day TTL
clawmacdo ark-api-key \
  --access-key "<YOUR_ACCESS_KEY>" \
  --secret-key "<YOUR_SECRET_KEY>" \
  --resource-ids ep-20260315233753-58rpv

# Multiple endpoints, 30-day TTL
clawmacdo ark-api-key \
  --access-key "<YOUR_ACCESS_KEY>" \
  --secret-key "<YOUR_SECRET_KEY>" \
  --resource-ids ep-20260315233753-58rpv,ep-20260310120000-abc12 \
  --duration 2592000

# Bot resource type
clawmacdo ark-api-key \
  --access-key "<YOUR_ACCESS_KEY>" \
  --secret-key "<YOUR_SECRET_KEY>" \
  --resource-type bot \
  --resource-ids bot-20260315-xyz \
  --duration 86400

# Using environment variables
export BYTEPLUS_ACCESS_KEY="<YOUR_ACCESS_KEY>"
export BYTEPLUS_SECRET_KEY="<YOUR_SECRET_KEY>"
clawmacdo ark-api-key \
  --resource-ids ep-20260315233753-58rpv \
  --duration 604800
```

**Sample Output:**

```
Generating BytePlus ARK API key...
  Resource type: endpoint
  Resource IDs:  ep-20260315233753-58rpv
  Duration:      604800 seconds (7.0 days)

ARK API Key generated successfully.
  API Key:  eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...
  Expires:  2026-03-22 15:42:46 UTC

Use this key as a Bearer token for ARK inference endpoints.
```

### Equivalent curl (GetApiKey)

```bash
# Note: This requires HMAC-SHA256 signing which is handled by the CLI.
# The raw API call is:

curl -X POST "https://open.byteplusapi.com/?Action=GetApiKey&Version=2024-01-01" \
  -H "Content-Type: application/json" \
  -H "X-Date: 20260315T154200Z" \
  -H "Authorization: HMAC-SHA256 Credential=<ACCESS_KEY>/20260315/ap-southeast-1/ark/request, SignedHeaders=x-date, Signature=<COMPUTED_SIGNATURE>" \
  -d '{
    "DurationSeconds": 604800,
    "ResourceType": "endpoint",
    "ResourceIds": ["ep-20260315233753-58rpv"]
  }'
```

**Response:**

```json
{
  "ResponseMetadata": {
    "RequestId": "20260315154200ABCDEF1234567890",
    "Action": "GetApiKey",
    "Version": "2024-01-01",
    "Service": "ark",
    "Region": "ap-southeast-1"
  },
  "Result": {
    "ApiKey": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...",
    "ExpiredTime": 1774194166
  }
}
```

### Equivalent curl (ListEndpoints)

```bash
curl -X POST "https://ark.ap-southeast-1.byteplusapi.com/?Action=ListEndpoints&Version=2024-01-01" \
  -H "Content-Type: application/json" \
  -H "X-Date: 20260315T154200Z" \
  -H "Authorization: HMAC-SHA256 Credential=<ACCESS_KEY>/20260315/ap-southeast-1/ark/request, SignedHeaders=x-date, Signature=<COMPUTED_SIGNATURE>" \
  -d '{
    "PageSize": 100,
    "PageNumber": 1
  }'
```

**Response:**

```json
{
  "ResponseMetadata": {
    "RequestId": "20260315154200ABCDEF1234567891",
    "Action": "ListEndpoints",
    "Version": "2024-01-01",
    "Service": "ark",
    "Region": "ap-southeast-1"
  },
  "Result": {
    "TotalCount": 2,
    "PageNumber": 1,
    "PageSize": 100,
    "Items": [
      {
        "Id": "ep-20260315233753-58rpv",
        "Name": "my-endpoint",
        "Status": "Running"
      }
    ]
  }
}
```

---

## ark-chat

Send a chat completion prompt to a BytePlus ARK model endpoint. Uses the OpenAI-compatible API.

### Syntax

```
clawmacdo ark-chat --api-key <KEY> --endpoint-id <ID> "<PROMPT>"
```

### Examples

```bash
# Direct usage
clawmacdo ark-chat \
  --api-key "<YOUR_ARK_API_KEY>" \
  --endpoint-id ep-20260315233753-58rpv \
  "Hello, what model are you?"

# Using environment variables
export ARK_API_KEY="<YOUR_ARK_API_KEY>"
export ARK_ENDPOINT_ID="ep-20260315233753-58rpv"
clawmacdo ark-chat "Explain quantum computing in 3 sentences."

# Longer prompt
clawmacdo ark-chat \
  --api-key "<YOUR_ARK_API_KEY>" \
  --endpoint-id ep-20260315233753-58rpv \
  "Write a Python function to check if a number is prime. Include docstring and type hints."
```

**Sample Output:**

```
I'm GLM (General Language Model), developed by Z.ai. I'm designed to be helpful,
informative, and safe while assisting users with various tasks.

[tokens: 12 prompt + 241 completion = 253 total]
```

### Equivalent curl

```bash
curl -X POST "https://ark.ap-southeast.bytepluses.com/api/v3/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <YOUR_ARK_API_KEY>" \
  -d '{
    "model": "ep-20260315233753-58rpv",
    "messages": [
      {"role": "user", "content": "Hello, what model are you?"}
    ]
  }'
```

**Response:**

```json
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion",
  "created": 1773589400,
  "model": "ep-20260315233753-58rpv",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "I'm GLM (General Language Model), developed by Z.ai..."
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 12,
    "completion_tokens": 241,
    "total_tokens": 253
  }
}
```

### Multi-turn Conversation (curl only)

```bash
curl -X POST "https://ark.ap-southeast.bytepluses.com/api/v3/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <YOUR_ARK_API_KEY>" \
  -d '{
    "model": "ep-20260315233753-58rpv",
    "messages": [
      {"role": "system", "content": "You are a helpful coding assistant."},
      {"role": "user", "content": "What is a closure in JavaScript?"},
      {"role": "assistant", "content": "A closure is a function that retains access to variables from its outer scope..."},
      {"role": "user", "content": "Give me an example."}
    ]
  }'
```

---

## serve

Start the web UI server for browser-based deployment management.

### Syntax

```
clawmacdo serve [--port <PORT>]
```

### Examples

```bash
# Default port (3456), localhost only
clawmacdo serve

# Custom port
clawmacdo serve --port 8080

# Allow remote access
CLAWMACDO_BIND=0.0.0.0 clawmacdo serve

# With authentication
CLAWMACDO_API_KEY="my-secret-key" CLAWMACDO_PIN="482617" clawmacdo serve

# Full production setup
CLAWMACDO_API_KEY="my-secret-key" \
  CLAWMACDO_PIN="482617" \
  CLAWMACDO_BIND="0.0.0.0" \
  clawmacdo serve --port 3456
```

**Sample Output:**

```
ClawMacToDO web UI running at http://127.0.0.1:3456
  (localhost only — set CLAWMACDO_BIND=0.0.0.0 to allow remote access)
  PIN protection enabled (CLAWMACDO_PIN)
  API key protection enabled (CLAWMACDO_API_KEY)
Press Ctrl+C to stop.
```

### Security

| Feature | Env Variable | Description |
|---------|-------------|-------------|
| API key | `CLAWMACDO_API_KEY` | Required for all `/api/*` endpoints (via `x-api-key` header or valid PIN session cookie) |
| PIN login | `CLAWMACDO_PIN` | 6-digit PIN for web UI login. Sets an HttpOnly session cookie on success |
| Bind address | `CLAWMACDO_BIND` | Bind interface. Default: `127.0.0.1` (localhost only). Set to `0.0.0.0` for remote access |
| Rate limiting | — | 60 requests/minute per IP address |
| CORS | — | Restricted to configured origins |

### Web UI Features

- **Deploy tab** — Deploy new OpenClaw instances to any of the 5 supported cloud providers
- **Deployments tab** — View all deployments, destroy instances, toggle Tailscale Funnel on/off
- **Funnel toggle** — Each deployment row has an On/Off button to enable/disable Tailscale Funnel, showing the public URL when active
- **Logout** — `/logout` clears the session cookie and redirects to the login page

---

## Environment Variables

All credentials can be set via environment variables instead of CLI flags.

### Cloud Provider Credentials

| Variable | Used by | Description |
|----------|---------|-------------|
| `DO_TOKEN` | deploy, destroy | DigitalOcean API token |
| `TENCENT_SECRET_ID` | deploy, destroy | Tencent Cloud Secret ID |
| `TENCENT_SECRET_KEY` | deploy, destroy | Tencent Cloud Secret Key |
| `AWS_ACCESS_KEY_ID` | deploy | AWS IAM access key ID |
| `AWS_SECRET_ACCESS_KEY` | deploy | AWS IAM secret access key |
| `AZURE_TENANT_ID` | deploy, destroy | Azure AD tenant ID |
| `AZURE_SUBSCRIPTION_ID` | deploy, destroy | Azure subscription ID |
| `AZURE_CLIENT_ID` | deploy, destroy | Azure service principal client ID |
| `AZURE_CLIENT_SECRET` | deploy, destroy | Azure service principal secret |
| `BYTEPLUS_ACCESS_KEY` | deploy, destroy, ark-api-key | BytePlus Access Key |
| `BYTEPLUS_SECRET_KEY` | deploy, destroy, ark-api-key | BytePlus Secret Key |
| `BYTEPLUS_ARK_API_KEY` | deploy | BytePlus ARK API Key (for model inference) |

### AI Model Keys

| Variable | Description |
|----------|-------------|
| `ANTHROPIC_API_KEY` | Anthropic Claude API key |
| `OPENAI_API_KEY` | OpenAI API key |
| `GEMINI_API_KEY` | Google Gemini API key |

### ARK Inference

| Variable | Used by | Description |
|----------|---------|-------------|
| `ARK_API_KEY` | ark-chat | BytePlus ARK bearer token |
| `ARK_ENDPOINT_ID` | ark-chat | ARK endpoint ID |

### Tailscale

| Variable | Used by | Description |
|----------|---------|-------------|
| `TAILSCALE_AUTH_KEY` | tailscale-funnel | Tailscale auth key (`tskey-auth-...`) |

### Skills API

| Variable | Used by | Description |
|----------|---------|-------------|
| `SKILLS_API_URL` | skill-upload, skill-download, skill-push | Base URL of the Railway skills API |
| `USER_SKILLS_API_KEY` | skill-upload, skill-download, skill-push | API key for user-skills endpoints |

### Web UI Server

| Variable | Used by | Description |
|----------|---------|-------------|
| `CLAWMACDO_API_KEY` | serve | API key for `/api/*` endpoint authentication |
| `CLAWMACDO_PIN` | serve | 6-digit PIN for web UI login |
| `CLAWMACDO_BIND` | serve | Bind address (default: `127.0.0.1`) |

---

## Web UI API Endpoints

When running `clawmacdo serve`, the following REST API endpoints are available.

> **Authentication:** When `CLAWMACDO_API_KEY` is set, all `/api/*` endpoints require either an `x-api-key` header or a valid PIN session cookie. Rate limited to 60 requests/minute per IP.

### POST /api/deploy

Start a new deployment.

```bash
curl -X POST http://localhost:3456/api/deploy \
  -H "Content-Type: application/json" \
  -d '{
    "provider": "digitalocean",
    "customer_name": "my-instance",
    "customer_email": "user@example.com",
    "do_token": "<YOUR_DO_TOKEN>",
    "anthropic_key": "<YOUR_ANTHROPIC_KEY>",
    "primary_model": "anthropic",
    "profile": "full"
  }'
```

**Response:**

```json
{
  "deploy_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

### GET /api/deploy/{id}/progress

Stream deployment progress via Server-Sent Events (SSE).

```bash
curl -N http://localhost:3456/api/deploy/a1b2c3d4-e5f6-7890-abcd-ef1234567890/progress
```

**Response (SSE stream):**

```
data: {"step":1,"total":16,"label":"Generate SSH key pair","status":"completed"}

data: {"step":2,"total":16,"label":"Upload SSH key to cloud","status":"running"}
```

### GET /api/deployments

List all tracked deployments.

```bash
curl http://localhost:3456/api/deployments
```

**Response:**

```json
[
  {
    "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "provider": "digitalocean",
    "hostname": "my-instance.openclaw.dev",
    "ip_address": "128.199.123.45",
    "status": "completed",
    "created_at": "2026-03-15T10:30:00Z"
  }
]
```

### POST /api/deployments/{id}/destroy

Destroy a deployed instance and remove the local record.

```bash
# DigitalOcean
curl -X POST http://localhost:3456/api/deployments/a1b2c3d4/destroy \
  -H "Content-Type: application/json" \
  -d '{"do_token": "<YOUR_DO_TOKEN>"}'

# Tencent Cloud
curl -X POST http://localhost:3456/api/deployments/a1b2c3d4/destroy \
  -H "Content-Type: application/json" \
  -d '{
    "tencent_secret_id": "<YOUR_SECRET_ID>",
    "tencent_secret_key": "<YOUR_SECRET_KEY>"
  }'

# BytePlus
curl -X POST http://localhost:3456/api/deployments/a1b2c3d4/destroy \
  -H "Content-Type: application/json" \
  -d '{
    "byteplus_access_key": "<YOUR_ACCESS_KEY>",
    "byteplus_secret_key": "<YOUR_SECRET_KEY>"
  }'

# Azure
curl -X POST http://localhost:3456/api/deployments/a1b2c3d4/destroy \
  -H "Content-Type: application/json" \
  -d '{
    "azure_tenant_id": "<YOUR_TENANT_ID>",
    "azure_subscription_id": "<YOUR_SUBSCRIPTION_ID>",
    "azure_client_id": "<YOUR_CLIENT_ID>",
    "azure_client_secret": "<YOUR_CLIENT_SECRET>"
  }'
```

**Response:**

```json
{
  "success": true,
  "message": "Instance destroyed and record removed."
}
```

### POST /api/deployments/{id}/funnel

Toggle Tailscale Funnel on or off for a deployment.

```bash
# Enable Funnel
curl -X POST http://localhost:3456/api/deployments/a1b2c3d4/funnel \
  -H "Content-Type: application/json" \
  -H "x-api-key: <YOUR_API_KEY>" \
  -d '{"action": "on", "port": 18789}'

# Disable Funnel
curl -X POST http://localhost:3456/api/deployments/a1b2c3d4/funnel \
  -H "Content-Type: application/json" \
  -H "x-api-key: <YOUR_API_KEY>" \
  -d '{"action": "off"}'
```

**Response (on):**

```json
{
  "ok": true,
  "message": "Funnel enabled at https://openclaw-ff54485d.tail12345.ts.net",
  "funnel_url": "https://openclaw-ff54485d.tail12345.ts.net"
}
```

**Response (off):**

```json
{
  "ok": true,
  "message": "Funnel disabled."
}
```

---

## Deploy Steps Reference

Every deployment tracks 16 steps in order:

| Step | Label |
|------|-------|
| 1 | Generate SSH key pair |
| 2 | Upload SSH key to cloud |
| 3 | Create cloud instance |
| 4 | Wait for instance to be active |
| 5 | Wait for SSH connectivity |
| 6 | Wait for cloud-init |
| 7 | Upload backup archive (skipped if no backup) |
| 8 | Create openclaw user |
| 9 | Install Node.js |
| 10 | Install OpenClaw |
| 11 | Configure environment (.env) |
| 12 | Run openclaw doctor |
| 13 | Run openclaw onboard |
| 14 | Install AI CLI tools |
| 15 | Configure systemd service |
| 16 | Start gateway |

---

## Local File Paths

| Path | Description |
|------|-------------|
| `~/.clawmacdo/` | Main application directory |
| `~/.clawmacdo/keys/` | SSH key pairs (per deployment) |
| `~/.clawmacdo/deploys/` | Deploy record JSON files |
| `~/.clawmacdo/backups/` | Backup archives |
| `~/.clawmacdo/deploy-<id>.log` | Detach mode deploy log |
| `~/.clawmacdo/clawmacdo.db` | SQLite tracking database |
