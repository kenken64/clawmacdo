# clawmacdo

[![Release](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/release.yml)
[![Changelog](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml/badge.svg)](https://github.com/kenken64/clawmacdo/actions/workflows/changelog.yml)

Rust CLI tool for deploying [OpenClaw](https://openclaw.ai) to **DigitalOcean** or **Tencent Cloud** — with Claude Code, Codex, and Gemini CLI pre-installed.

## ✨ Latest Update (March 2026)

**🏗️ Major Refactor Complete:** ClawMacdo has been refactored from a monolithic structure into a **modular workspace architecture** with focused crates for better maintainability, testing, and performance.

### 🚀 New Architecture Benefits
- **Modular design** - Each crate has a single responsibility
- **Feature flags** - Build only what you need (minimal, web-ui, cloud providers)
- **32% smaller binaries** - Optimized builds from 4.6MB → 3.1MB
- **Faster compilation** - Incremental builds only rebuild changed crates
- **Better testing** - Isolated crate testing

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
| **clawmacdo-cloud** | DigitalOcean & Tencent Cloud APIs | reqwest, async-trait |
| **clawmacdo-provision** | Server setup, package installation | SSH, Core, UI |
| **clawmacdo-db** | SQLite operations, job tracking | rusqlite |
| **clawmacdo-ssh** | SSH connections, file transfers | ssh2 |
| **clawmacdo-ui** | Progress bars, web interface | indicatif, axum |

## Features

- **Multi-cloud**: Deploy to DigitalOcean or Tencent Cloud with `--provider` flag
- **Backup** local `~/.openclaw/` config into a timestamped `.tar.gz`
- **1-click deploy**: generate SSH keys, provision a cloud instance, install Node 24 + OpenClaw + Claude Code + Codex + Gemini CLI, restore config, configure `.env` (API + messaging), start the gateway, and auto-configure model failover
- **Cloud-to-cloud migration**: SSH into a source instance, back up remotely, deploy to a new instance, restore
- **Destroy**: delete an instance by name with confirmation, clean up SSH keys (cloud + local)
- **Status**: list all openclaw-tagged instances with IPs
- **List backups**: show local backup archives with sizes and dates
- **Web UI**: Browser-based deploy interface with real-time SSE progress streaming (optional)
- **Security groups**: Auto-create firewall rules on Tencent Cloud (SSH + HTTP/HTTPS)

## Supported Cloud Providers

| Provider | Flag | Credentials |
|----------|------|-------------|
| DigitalOcean | `--provider=digitalocean` (default) | `--do-token` |
| Tencent Cloud | `--provider=tencent` | `--tencent-secret-id` + `--tencent-secret-key` |

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

## Build Features

| Feature | Description | Default |
|---------|-------------|---------|
| `web-ui` | Browser-based deployment interface | ✅ |
| `tencent-cloud` | Tencent Cloud provider support | ✅ |
| `digitalocean` | DigitalOcean provider support | ✅ |
| `minimal` | CLI-only, no web UI or optional features | ❌ |

## Usage

### Deploy OpenClaw to DigitalOcean

```bash
# Set your DO token
export DO_TOKEN="your_digitalocean_api_token"

# Deploy with backup & restore
clawmacdo deploy \
  --customer-name "my-openclaw" \
  --restore-from ~/backups/openclaw-backup-2024-03-09.tar.gz
```

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
  --size s-2vcpu-4gb \
  --region nyc1 \
  --restore-from ~/openclaw-backup.tar.gz \
  --claude-api-key "$CLAUDE_API_KEY" \
  --openai-api-key "$OPENAI_API_KEY" \
  --whatsapp-phone "+1234567890" \
  --telegram-token "$TELEGRAM_TOKEN" \
  --tailscale \
  --tailscale-auth-key "$TAILSCALE_AUTH"
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
| `TENCENT_SECRET_ID` | Tencent Cloud Secret ID | For Tencent deploys |
| `TENCENT_SECRET_KEY` | Tencent Cloud Secret Key | For Tencent deploys |
| `CLAUDE_API_KEY` | Anthropic Claude API key | Optional |
| `OPENAI_API_KEY` | OpenAI API key | Optional |
| `TELEGRAM_TOKEN` | Telegram bot token | Optional |
| `TAILSCALE_AUTH_KEY` | Tailscale auth key | Optional |

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

**Last updated:** March 9, 2026  
**Architecture version:** 2.0 (modular workspace)  
**Binary optimizations:** ✅ Applied (32% size reduction)