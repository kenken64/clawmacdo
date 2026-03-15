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
# Default port (3456)
clawmacdo serve

# Custom port
clawmacdo serve --port 8080
```

**Sample Output:**

```
ClawMacdo Web UI running at http://localhost:3456
```

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

---

## Web UI API Endpoints

When running `clawmacdo serve`, the following REST API endpoints are available:

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
