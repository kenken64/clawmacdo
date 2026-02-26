# clawmacdo

[![Release](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml)
[![Changelog](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml)

Rust CLI tool for migrating [OpenClaw](https://openclaw.ai) from Mac or an existing DigitalOcean droplet to a new DigitalOcean droplet — with Claude Code and Codex pre-installed.

## Features

- **Backup** local `~/.openclaw/` config into a timestamped `.tar.gz`
- **1-click deploy**: generate SSH keys, provision a DO droplet, install Node 24 + OpenClaw + Claude Code + Codex, restore config, configure `.env` (API + messaging), start the gateway
- **DO-to-DO migration**: SSH into a source droplet, back up remotely, deploy to a new droplet, restore
- **Destroy**: delete a droplet by name with confirmation, clean up SSH keys (DO + local)
- **Status**: list all `openclaw`-tagged droplets with IPs
- **List backups**: show local backup archives with sizes and dates

## Download

Pre-built binaries for every release are available on the [Releases page](https://github.com/kenken64/clawmacdo/releases):

| Platform | Architecture | File |
|----------|-------------|------|
| Windows  | x86_64      | `clawmacdo-windows-amd64.zip` |
| Linux    | x86_64      | `clawmacdo-linux-amd64.tar.gz` |
| macOS    | Apple Silicon (arm64) | `clawmacdo-darwin-arm64.tar.gz` |

## Installation

### From release binary

Download the archive for your platform from [Releases](https://github.com/kenken64/clawmacdo/releases), extract, and add to your `PATH`.

### From source

```bash
cargo build --release
```

The binary will be at `target/release/clawmacdo.exe` (Windows) or `target/release/clawmacdo` (Linux/macOS).

#### Build prerequisites

- Rust toolchain (stable)
- On Windows: MSVC build tools + Windows SDK (for `libssh2` native compilation)
- On Linux: `libssl-dev`, `pkg-config`

## Usage

```
clawmacdo <COMMAND>

Commands:
  backup        Archive ~/.openclaw/ and LaunchAgent plist into a .tar.gz
  deploy        Full 1-click deploy to DigitalOcean
  destroy       Destroy a droplet by name and clean up SSH keys
  migrate       DO → DO migration: backup source, deploy new, restore
  status        List deployed openclaw-tagged droplets
  list-backups  Show local backup archives
  help          Print help
```

### Backup

```bash
clawmacdo backup
```

Creates `~/.clawmacdo/backups/openclaw_backup_<timestamp>.tar.gz`.

### Deploy

```bash
clawmacdo deploy \
  --do-token=dop_v1_xxx \
  --anthropic-key=sk-ant-xxx \
  --openai-key=sk-xxx \
  --gemini-key=AIzaSy... \
  --whatsapp-phone-number=15551234567 \
  --telegram-bot-token=123456789:AA...
```

Optional flags: `--region` (default: `sgp1`), `--size` (default: `s-2vcpu-4gb`), `--hostname`, `--backup <path>`, `--enable-backups`.

Missing values trigger interactive prompts.

#### Deploy flow (12 steps)

```
 1. Resolve parameters (interactive prompts for missing values)
 2. Generate Ed25519 SSH key pair → ~/.clawmacdo/keys/
 3. Upload public key to DigitalOcean
 4. Create droplet with cloud-init (tagged "openclaw")
 5. Poll until droplet is active (5min timeout)
 6. Wait for SSH to accept connections (2min timeout)
 7. Wait for cloud-init to complete (10min timeout)
 8. SCP backup archive to server (if selected)
 9. Extract configs into ~/.openclaw/, preserve .env
10. Start OpenClaw gateway via systemd
11. Save deploy record to ~/.clawmacdo/deploys/
12. Print provisioning summary
```

### Migrate (DO to DO)

```bash
clawmacdo migrate \
  --do-token=dop_v1_xxx \
  --anthropic-key=sk-ant-xxx \
  --openai-key=sk-xxx \
  --whatsapp-phone-number=15551234567 \
  --telegram-bot-token=123456789:AA... \
  --source-ip=164.90.x.x \
  --source-key=~/.ssh/id_ed25519
```

Connects to the source droplet, creates a remote backup, downloads it locally, then runs the full deploy flow on a new droplet with the backup auto-selected.

### Resulting .env on server

After deploy/migrate, credentials and messaging settings are written to:

`/root/.openclaw/.env`

```bash
ANTHROPIC_API_KEY=...
OPENAI_API_KEY=...
GEMINI_API_KEY=...
WHATSAPP_PHONE_NUMBER=...
TELEGRAM_BOT_TOKEN=...
```

### Destroy

```bash
clawmacdo destroy \
  --do-token=dop_v1_xxx \
  --name=openclaw-8d533bfd
```

Finds the named droplet among `openclaw`-tagged droplets, shows its details (name, IP, region), and asks for confirmation before destroying. Also cleans up:

- The associated SSH key from your DigitalOcean account (`clawmacdo-<hostname_suffix>`)
- The local key file from `~/.clawmacdo/keys/`

### Status

```bash
clawmacdo status --do-token=dop_v1_xxx
```

### List Backups

```bash
clawmacdo list-backups
```

## What gets installed on the droplet

1. System packages: `curl`, `gnupg`, `ufw`, `git`, `build-essential`
2. Firewall (UFW): ports 22 (SSH) and 18789 (OpenClaw gateway) only
3. Node.js 24 LTS via NodeSource
4. OpenClaw gateway
5. Claude Code CLI (`@anthropic-ai/claude-code`)
6. Codex CLI (`@openai/codex`)
7. API keys and messaging config written to `/root/.openclaw/.env` (Anthropic, OpenAI, Gemini, WhatsApp phone number, Telegram bot token)

### Self-healing & resilience

Every deployed droplet includes automatic recovery mechanisms:

| Feature | Description |
|---------|-------------|
| **loginctl linger** | Enabled for root — gateway survives SSH disconnects |
| **Health-check script** | `/root/.openclaw/workspace/openclaw-healthcheck.sh` — checks gateway process + RPC probe |
| **Cron: health-check** | Runs every 5 minutes (`*/5 * * * *`), auto-restarts gateway on failure |
| **Cron: log rotation** | Truncates health-check log daily at midnight to prevent disk fill |
| **Double-check restart** | Health-check retries after 15s before restarting to avoid false positives |

> **Note:** OpenClaw's installer creates its own user-level systemd service at `~/.config/systemd/user/openclaw-gateway.service`. The cloud-init script does not create a competing systemd unit — it only prepares the environment and resilience tooling.

### API key validation

The deploy process validates Anthropic API keys before writing them to `.env`:

| Key prefix | Type | Action |
|-----------|------|--------|
| `sk-ant-api-...` | Real API key | ✅ Written to `.env` |
| `sk-ant-oat-...` | OAuth session token | ❌ Filtered out — empty string written |
| _(empty)_ | Not provided | ⚠️ Skipped |

**Why?** When you authenticate via `openclaw login`, OpenClaw stores an OAuth session token (`sk-ant-oat-...`) in `openclaw.json`. If this token gets backed up and restored to a new instance, the gateway injects it as `ANTHROPIC_API_KEY` into child processes. Claude Code expects a real API key and fails with auth errors.

The fix: clawmacdo now detects OAuth tokens and refuses to write them to `.env`. The OpenClaw gateway is unaffected — it manages its own auth via `openclaw.json` profiles. A warning is printed during deploy if an OAuth token is detected.

## Environment variables

Credentials and messaging settings can be passed as flags or environment variables:

| Flag | Env var | Required |
|---|---|---|
| `--do-token` | `DO_TOKEN` | ✅ Yes |
| `--anthropic-key` | `ANTHROPIC_API_KEY` | ✅ Yes |
| `--openai-key` | `OPENAI_API_KEY` | Optional |
| `--gemini-key` | `GEMINI_API_KEY` | Optional |
| `--whatsapp-phone-number` | `WHATSAPP_PHONE_NUMBER` | Optional |
| `--telegram-bot-token` | `TELEGRAM_BOT_TOKEN` | Optional |

## Data directories

| Path | Purpose |
|---|---|
| `~/.clawmacdo/backups/` | Backup archives |
| `~/.clawmacdo/keys/` | Generated SSH key pairs |
| `~/.clawmacdo/deploys/` | Deploy record JSON files |

## Project structure

```
src/
├── main.rs              # Clap CLI entry point
├── commands/
│   ├── mod.rs
│   ├── backup.rs        # Scan + tar.gz ~/.openclaw/
│   ├── deploy.rs        # 12-step deploy orchestrator
│   ├── migrate.rs       # DO→DO: remote backup + deploy
│   ├── destroy.rs       # Destroy droplet + clean up SSH keys
│   ├── status.rs        # DO API → list tagged droplets
│   └── list_backups.rs  # List local backup files
├── config.rs            # App paths, constants, DeployRecord
├── digitalocean.rs      # DO API client
├── ssh.rs               # Ed25519 keygen, SSH exec, SCP
├── cloud_init.rs        # Cloud-init YAML template (includes healthcheck + linger)
├── ui.rs                # Interactive prompts, spinners, summary
└── error.rs             # Typed errors (thiserror)
```

## License

MIT
