# clawmacdo

Rust CLI tool for migrating [OpenClaw](https://openclaw.ai) from Mac or an existing DigitalOcean droplet to a new DigitalOcean droplet — with Claude Code and Codex pre-installed.

## Features

- **Backup** local `~/.openclaw/` config into a timestamped `.tar.gz`
- **1-click deploy**: generate SSH keys, provision a DO droplet, install Node 24 + OpenClaw + Claude Code + Codex, restore config, start the gateway
- **DO-to-DO migration**: SSH into a source droplet, back up remotely, deploy to a new droplet, restore
- **Status**: list all `openclaw`-tagged droplets with IPs
- **List backups**: show local backup archives with sizes and dates

## Installation

```bash
cargo build --release
```

The binary will be at `target/release/clawmacdo.exe` (Windows) or `target/release/clawmacdo` (Linux/macOS).

### Prerequisites

- Rust toolchain (stable)
- On Windows: MSVC build tools + Windows SDK (for `libssh2` native compilation)

## Usage

```
clawmacdo <COMMAND>

Commands:
  backup        Archive ~/.openclaw/ and LaunchAgent plist into a .tar.gz
  deploy        Full 1-click deploy to DigitalOcean
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
  --openai-key=sk-xxx
```

Optional flags: `--region` (default: `sgp1`), `--size` (default: `s-2vcpu-4gb`), `--hostname`, `--backup <path>`, `--enable-backups`.

Missing values trigger interactive prompts.

### Migrate (DO to DO)

```bash
clawmacdo migrate \
  --do-token=dop_v1_xxx \
  --anthropic-key=sk-ant-xxx \
  --openai-key=sk-xxx \
  --source-ip=164.90.x.x \
  --source-key=~/.ssh/id_ed25519
```

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
7. API keys written to `/root/.openclaw/.env`
8. Systemd service: `openclaw-gateway.service`

## Environment variables

Tokens can be passed as flags or environment variables:

| Flag | Env var |
|---|---|
| `--do-token` | `DO_TOKEN` |
| `--anthropic-key` | `ANTHROPIC_API_KEY` |
| `--openai-key` | `OPENAI_API_KEY` |

## Data directories

| Path | Purpose |
|---|---|
| `~/.clawmacdo/backups/` | Backup archives |
| `~/.clawmacdo/keys/` | Generated SSH key pairs |
| `~/.clawmacdo/deploys/` | Deploy record JSON files |

## License

MIT
