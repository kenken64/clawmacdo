# Changelog

## v0.93.0

### Added
- **`ttyproxy-instance-set` subcommand** — set the `OPENCLAW_INSTANCE` value in the claw-ttyproxy project `.env` (`/home/openclaw/.openclaw/workspace/claw-ttyproxy/.env`) on a deployed instance (`--instance` + `--openclaw-instance`, optional `--env-file` for a different workspace path). Replaces the existing `OPENCLAW_INSTANCE=` line in place or appends it if missing, quotes the value safely, refuses symlinked or outside-workspace `.env` paths, and writes a timestamped backup beside the file. Restart the tty proxy afterwards to pick up the change
- **`gyne-consumer-profile` now restarts the consumer** — after writing `CONSUMER_NAME`/`CONSUMER_TASK_STREAM` to `.env`, the command restarts the Gyne consumer `systemctl --user` service by default so the worker re-registers under the new name (editing `.env` alone does not affect a running process). The unit is auto-detected from the enabled `--user` services (a `gyne`/`consumer` unit, excluding the gateway); pass `--service <unit>` to target a specific unit or `--no-restart` to only edit `.env`
- **`gyne-consumer-profile` subcommand** — update `workspace/gyne-agent/.env` on a deployed OpenClaw instance (`--instance` + `--name`, optional `--agent`, `--project`, `--task-stream`, `--json`) by changing `CONSUMER_NAME` and the matching `CONSUMER_TASK_STREAM` while preserving the rest of the file and writing a backup beside it
- **`gws-login` subcommand** — install Google Workspace (`gws`) credentials on a deployed instance (`--instance` + `--filename`, default `credentials.json`). `gws auth login` is an interactive browser OAuth flow with no headless mode and the instance is headless, so credentials are *injected* rather than minted on the box. Two input modes:
  - `--credentials <file>`: a local gws JSON (e.g. from `gws auth export --unmasked`, or produced by an external OAuth flow), validated as JSON.
  - `--code <oauth-code>` with `--client-id` / `--client-secret` / `--redirect-uri` (plus `--code-verifier` when the flow used PKCE): clawmacdo exchanges the authorization code at Google's token endpoint and builds the standard `authorized_user` credentials JSON. This avoids the common masked-export pitfall (`gws auth export` without `--unmasked` writes `***` values that gws can't parse). It fails loudly if Google returns no `refresh_token`, since that means the auth request lacked `access_type=offline` and the agent would de-authenticate within ~1 hour.

  Either way the resulting JSON is base64-encoded and written over a single SSH session to `~/.config/gws/` as the `openclaw` user with `0600` permissions; a best-effort `gws auth status` confirms gws accepted it
- **`gws-logout` subcommand** — log out Google Workspace on a deployed instance (`--instance`): runs `gws auth logout` (revokes the token with Google + clears it), then removes local `credentials.json`/`token.json` as a fallback while preserving `client_secret.json` so a future login needs no `gws auth setup` re-run

## v0.87.0

### Added
- **Hermes Agent Lightsail provisioning** - new `hermes-provision` / `provision-hermes` command provisions Hermes Agent on AWS Lightsail with Docker, hardened SSH/UFW defaults, deploy records, dashboard support, and dry-run output.
- **Hermes deployment path in the web UI** - after PIN unlock, deployment cards now let users choose OpenClaw or Hermes Agent; Hermes deployments use AWS Lightsail credentials, regions, sizes, and Hermes-specific progress/summary actions.
- **Hermes Telegram onboarding** - CLI and web UI collect Telegram bot token, allowed user IDs, and home channel ID, then write the matching Hermes environment variables during provisioning.
- **AWS Bedrock Nova Pro onboarding for Hermes** - Hermes provisioning now requires a Bedrock API key, defaults to Nova Pro (`amazon.nova-pro-v1:0`) in Singapore (`ap-southeast-1`), writes the Bedrock Mantle endpoint, and configures AWS CLI region files on the instance.

### Changed
- **Deployment copy and branding** - web UI copy and artwork now reflect both OpenClaw and Hermes Agent deployment support.

## v0.86.0

### Added
- **Claude Code reconnect flow** - `claude-auth-start --json` SSHes into an OpenClaw instance, starts Claude Code auth in a remote PTY, captures the generated login URL, and returns it for 2ndBrain; `claude-auth-status --json` gives a pollable authenticated/pending JSON contract.

## v0.82.0

### Added
- **Runtime thread controls** - clawmacdo now honors `CLAWMACDO_TOKIO_WORKER_THREADS`, `CLAWMACDO_MAX_CONCURRENCY`, `CLAWMACDO_RAYON_NUM_THREADS`, `CLAWMACDO_SPAWN_RETRIES`, and `CLAWMACDO_SPAWN_RETRY_DELAY_MS` so hosted apps can cap internal Rust thread creation and retry detached child process spawns.

## v0.81.0

### Added
- **AI wiki ingestion** - `wiki-ingest` uploads converted Markdown into a project wiki, runs Claude Code on the OpenClaw instance to decide where the content belongs, updates related pages/index/log, and returns structured JSON with source and changed files.

## v0.80.0

### Added
- **Persistent state directory override** - `CLAWMACDO_STATE_DIR` controls where deploy records, SSH keys, backups, and the web UI database are stored; Railway `.clawmacdo` volumes are auto-detected via `RAILWAY_VOLUME_MOUNT_PATH`.
- **Railway web UI container config** - root `Dockerfile` and `railway.toml` build the Rust CLI image and start `clawmacdo serve` on Railway's `$PORT` instead of a missing `/app/server.js`.
- **Fast Lightsail restore workflow** - `ls-restore-fast` restores a Lightsail snapshot, waits for SSH, batches Telegram/identity/Remotion `.env` updates into one remote session, restarts the gateway once, and checks Tailscale plus cloudflared in parallel.

## v0.79.0

### Fixed
- **Lightsail restore Telegram recovery** - `ls-restore` now normalizes legacy Telegram streaming config, runs `openclaw doctor --fix`, restarts the restored gateway, and waits for health so Telegram pairing codes can be generated after snapshot restore.
- **Telegram setup config compatibility** - `telegram-setup` no longer writes the legacy `channels.telegram.streaming` string value that newer OpenClaw gateways reject.
- **Web pairing approval on Lightsail** - deployment summary pairing approval now carries the cloud provider so Lightsail approvals use the `ubuntu` SSH path.

## v0.78.0

### Added
- **Safe wiki project deletion** - `wiki-delete` removes only direct `workspace/wiki-*` project folders and rejects absolute paths, traversal, symlinks, files, empty names, and non-`wiki-*` slugs.

## v0.77.0

### Added
- **Project-scoped LLM wiki setup** - `openclaw-llm-wiki` now accepts `--project <slug>` and `--json`, creates/refines `workspace/<project>/`, and returns structured Claude status/error details for web apps.

## v0.76.0

### Changed
- **Faster Telegram and identity setup** - `telegram-setup` and `openclaw-identity` now update remote OpenClaw config files directly and use bounded gateway health polling instead of invoking slower remote OpenClaw CLI setup paths.

## v0.75.0

### Added
- **Project wiki file commands** - `wiki-tree`, `wiki-index`, `wiki-read`, `wiki-write`, and `wiki-export` let web apps safely list, index, read, edit, and export Markdown wiki files from an OpenClaw workspace.
- **Gateway URL JSON command** - `openclaw-gateway-url --json` returns the current public Gateway URL when Tailscale Funnel is available.
- **Remotion avatar JSON output** - `remotion-avatar-setup --json` returns a stable `remotion_url` object for app integrations.

### Security
- **Wiki path allowlisting and revision checks** - wiki commands reject absolute/escaping paths, only operate on `.md` files under the resolved OpenClaw workspace, return SHA-256 hashes, and require `--base-sha` for writes.

## v0.74.0

### Fixed
- **Lightsail snapshot parsing** - accept both numeric and string `createdAt` values from AWS CLI snapshot responses, including timezone-aware timestamps such as `2026-05-10T11:57:50.432000+08:00`.
- **Lightsail snapshot list timestamps** - show parsed Lightsail snapshot creation time in the dashboard snapshot table instead of dropping the field.

## v0.73.0

### Added
- **`openclaw-llm-wiki` subcommand** - create an attachable `llm_wiki.md` in the active OpenClaw workspace, seed a `llm_wiki/` project tree, and optionally launch Claude Code to refine it.
- **LLM wiki Markdown upload** - `openclaw-llm-wiki --llm-wiki-md <path>` uploads any local Markdown file into the target workspace as `llm_wiki.md`; `--skip-claude` supports upload-only flows.
- **Dashboard LLM wiki upload action** - deployments now expose an `Upload LLM Wiki` action that sends a `.md` file through the local web UI to the selected OpenClaw instance.

### Changed
- **Markdown context exports** - `openclaw-md-download` now includes `llm_wiki.md` when present.

## v0.72.0

### Added
- **Remotion avatar OpenAI and voice options** - `remotion-avatar-setup` now accepts `--openai-api-key`/`--open-api-key` and `--voice-gender male|female`, writing `OPENAI_API_KEY`, `VOICE_GENDER`, and `TTS_VOICE` into the Remotion app `.env`.
- **Remotion avatar GLB upload** - `remotion-avatar-setup --avatar-glb <path>` uploads `avatar.glb` or `<userid>_avatar.glb` to the Remotion app as `public/avatar.glb`.
- **Tailscale deploy form regression coverage** - Playwright now covers entering the Tailscale auth key before enabling the Tailscale checkbox.

### Fixed
- **Tailscale auth key UI validation** - the deploy form now detects a typed Tailscale auth key immediately, auto-enables the Tailscale option, and uses one value reader for validation and deploy payloads.

## v0.71.0

### Added
- **`openclaw-identity` subcommand** - assign an OpenClaw display name and owner context, write `IDENTITY.md`/`USER.md`, and restart the gateway.
- **`openclaw-md-download` subcommand** - download OpenClaw operating markdown files plus `memory/**/*.md` into a local zip archive.
- **`openclaw-gateway-token` subcommand** - regenerate `gateway.auth.token`, keep password auth in sync when configured, back up `openclaw.json`, and restart the gateway.
- **`remotion-avatar-setup` subcommand** - configure the Remotion 3D AI avatar app with OpenClaw chat credentials, set `VITE_AVATAR_NAME`, replace `kenken64`, install `cloudflared`, start systemd user services, and return the Quick Tunnel URL.

### Fixed
- **Node.js/Claude Code provisioning** - install AI CLIs with optional dependencies and repair the Claude native binary when postinstall did not run.
- **Remotion avatar setup reliability** - prevent Claude from consuming the remote setup script, write OpenAI-compatible environment aliases for existing app code, tunnel the Vite frontend port, and capture the Cloudflare URL from a service log file.

## v0.70.0

### Added
- **OpenCode support** — new `opencode` model option for deploy and update-model commands. When `--primary-model opencode` is selected, the deploy installs OpenCode via `curl -fsSL https://opencode.ai/install | bash` and configures MiniMax M2.5 Free (`opencode/minimax-m2.5-free`) as the default model. No API key required — the free model works out of the box.
- **`memory-download` subcommand** — download all memory archive files from an OpenClaw instance as a tar.gz archive (`--instance` + optional `--output`); SSHes into the instance, tars the memory directory, SCP-downloads it locally, and cleans up the remote temp file
- **`scp_download_as` SSH helper** — supports SCP file downloads as a specific SSH user (e.g. `ubuntu` for Lightsail instances)

## v0.67.1

### Fixed
- **Cron commands fail with "pairing required" on loopback** — `cron-list`, `cron-message`, `cron-tool`, and `cron-remove` now auto-approve any pending device pairing requests (via `openclaw devices approve`) before executing the cron command. This fixes the chicken-and-egg problem where the gateway rejects local CLI WebSocket connections from unrecognised devices. Deploy also auto-approves pending devices after gateway start.

### Changed
- **WhatsApp status checks now read `creds.json`** instead of calling the Gateway REST API (which doesn't exist). Status is derived from `~/.openclaw/credentials/whatsapp/default/creds.json`: `me.id` + `registered` field. Reports `connected` (paired & registered), `pending` (paired but `registered: false`), or `not_paired` (no credentials). Applies to CLI `whatsapp-status`, `whatsapp-wait`, and `GET /api/deployments/{id}/whatsapp/status`.
- **Status response** now includes `name` and `registered` fields instead of `since`

## v0.67.1

### Added
- **`telegram-chat-id` subcommand** — retrieve the Telegram chat ID from a deployed instance by searching openclaw credentials and data directories via SSH
- **`telegram-reset` subcommand** — clear all Telegram pairing state (allowFrom, pairing credentials, update offsets) and restart the gateway so the bot prompts for a fresh pairing code
- **`whatsapp-reset` subcommand** — clear WhatsApp session credentials and restart the gateway so a new QR code scan is required for re-pairing
- **`--reset` flag for `telegram-setup` and `whatsapp-setup`** — combine reset + setup into a single SSH session, eliminating the extra connection from running reset and setup separately
- **`openclaw-versions` subcommand** — list all available OpenClaw versions from the npm registry (`--json` for machine-readable output)
- **`openclaw-install` subcommand** — install a specific OpenClaw version on a running instance (`--instance` + `--version`), then restart the gateway
- **`--openclaw-version` deploy flag** — pin a specific OpenClaw version during deployment instead of always installing `@latest`; defaults to latest if omitted
- **Web UI version selector** — deploy form now includes an OpenClaw version dropdown populated from the npm registry; new `GET /api/openclaw-versions` endpoint
- **`skill-remove` subcommand** — delete a deployed skill directory from an instance workspace by name (`--instance` + `--skill`); restarts the gateway after removal
- **`skill-diff` subcommand** — compare a local skill directory against the deployed skill on an OpenClaw instance (`--instance` + `--dir`): walks both sides using SHA-256 checksums and prints a drift report with ✓ in-sync, ≠ modified, + new locally, − only on instance; also shows gateway skill status

### Fixed
- **Telegram bot not polling after deploy** — three root causes identified and fixed for OpenClaw 2026.3.24:
  1. `OPENCLAW_BUNDLED_PLUGINS_DIR` in the systemd drop-in prevented channel initialisation entirely; removed from all providers (`deploy`, `docker-fix`, `whatsapp` commands)
  2. `TELEGRAM_BOT_TOKEN` in gateway.env/service env was not sufficient — token must be registered in `openclaw.json` via `openclaw channels add`; deploy now runs this automatically for all 5 cloud providers
  3. Stale `plugins.entries` (byteplus/telegram) in `openclaw.json` caused the gateway to spin at 100% CPU during hot-reload, abandoning port 18789; gateway restart after `openclaw models set` (added in v0.50) prevents the stuck hot-reload
- **Gateway port 18789 lost after model set** — `openclaw models set` triggered an internal hot-reload that got stuck; fixed by forcing `systemctl --user restart` after model configuration (all providers)

### Performance
- **`telegram-pair` non-blocking notification** — pairing approval completes immediately; the `--notify` Telegram API call is fired in the background, eliminating the Telegram API roundtrip from the CLI response time
- **`telegram-setup` step validation** — each step now pre-validates the previous step's outcome before proceeding: plugin enable checks token is in `gateway.env`, gateway restart checks token is present; each step emits a `ok`/`FAILED` status line and exits non-zero on failure so the sequence aborts immediately; blind `sleep 2` replaced with adaptive health check (same as deploy)
- **Deploy Step 15 gateway startup optimization** — health check loop reduced from 150 fixed 1s polls (max 150s) to 30 iterations with exponential backoff (1s→2s→3s, max ~70s, exits immediately on healthy); blind `sleep 2` after Telegram/model gateway restart replaced with the same adaptive health check; model setup and profile setup commands batched into a single SSH session (one TCP connect + handshake instead of two) across all 5 cloud providers (DigitalOcean, Tencent, BytePlus, Lightsail, Azure)
- **`skill-deploy` single-session optimization** — SCP upload, extraction, and gateway restart now share one SSH session (was two separate connections); extraction uses `unzip` instead of Python; permissions fixed in one `chmod -R` pass instead of two `find` walks; gateway restart polls for readiness instead of a fixed 2s sleep
- **`skill-list` subcommand** — list all skill directories deployed on an instance, resolved against the gateway-registered skill name from each `SKILL.md`, with readiness status
- **`skill-check-perms` subcommand** — audit file ownership and permissions for a deployed skill (`--instance` + `--skill`); reports any files not owned by `openclaw:openclaw` or with incorrect permissions (dirs `755`, files `644`); `--fix` flag auto-corrects in place

## v0.46.4

### Added
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
- **`cron-message` subcommand** — schedule a recurring message to the OpenClaw gateway agent; the agent processes it and delivers the response to Telegram, WhatsApp, or any other connected channel (uses `openclaw cron add` under the hood)
- **`cron-tool` subcommand** — schedule recurring tool execution on a deployed instance; the agent runs the named tool and announces the result to the chosen channel
- **`cron-list` subcommand** — list all cron jobs on a deployed instance
- **`cron-remove` subcommand** — remove a cron job by name from a deployed instance
- **`whatsapp-setup` subcommand** — set up WhatsApp on a deployed instance (set phone number, enable plugin, restart gateway, fetch pairing QR code)
- **`whatsapp-qr` subcommand** — fetch the WhatsApp pairing QR code from a deployed instance (re-fetch if expired)
- **`plugin-install` subcommand** — install OpenClaw plugins on deployed instances via `clawmacdo plugin-install --instance <id> --plugin @openguardrails/moltguard` (installs via pnpm, enables plugin, restarts gateway)
- **Windows PowerShell scripts** — all shell scripts now have `.ps1` equivalents for Windows support (`release.ps1`, `npm-package.ps1`, `npm-publish.ps1`, scan scripts, etc.)
- **Agent Docker Access warning** — deploy form shows the common Docker socket permission error with a clear fix instruction
- **Dual license** — switched from MIT to GPLv3 (open source) + Commercial (proprietary) dual license model

### Fixed
- **Docker fix: systemd user manager restart** — "Fix Agent Docker Access" now restarts the systemd user service manager so the gateway picks up the docker group
- **`KillMode=control-group`** — gateway service now kills the entire cgroup on restart, preventing orphaned child processes from holding the port
- **AWS credential passthrough** — web UI credentials are written to `~/.aws/credentials` so the AWS CLI uses them instead of stale local config
- **Lightsail destroy with credentials** — destroy modal now prompts for AWS Access Key ID and Secret Access Key
- **Lightsail snapshot listing** — credentials from the web UI are now passed through to the AWS CLI for snapshot listing

## v0.44.4

### Added
- **`skill-deploy` subcommand** — upload a `.zip` archive of OpenClaw skills to a deployed instance (`--instance` + `--file`): SCPs the archive, extracts it into `~/.openclaw/workspace/`, fixes ownership/permissions, and restarts the gateway in one step

## v0.46.4

### Fixed
- **No spurious "Azure/AWS CLI not found" warning on non-deploy commands** — the startup preflight check ran `ensure_az_cli()` and `ensure_aws_cli()` on every invocation (including `telegram-setup`, `telegram-pair`, etc.). Both functions are already called inside the relevant deploy handlers, so the redundant startup check has been removed.

## v0.46.4

### Fixed
- **`telegram-setup` now updates `gateway.env`** — the systemd service loads credentials from `gateway.env` via `EnvironmentFile`; previously only `.env` was updated, so the running gateway kept polling with the old bot token. Both files are now updated atomically so the restarted gateway picks up the new token immediately.
- **`telegram-setup` resets pairing state on re-run** — clears `telegram-pairing.json` and `update-offset-*.json` before applying the new bot token, so stale pairing requests from a previous bot are removed and users get a fresh pairing flow with the new bot.

## v0.46.4

### Added
- **`do-snapshot` subcommand** — create a named DigitalOcean snapshot from an existing droplet by ID (`--do-token` + `--droplet-id` + `--snapshot-name`), with optional `--power-off` flag for clean shutdown/snapshot/power-on cycle
- **DigitalOcean action polling API** — `shutdown_droplet()`, `power_on_droplet()`, `create_snapshot()`, `get_action()`, `wait_for_action()`, and `get_droplet_snapshots()` methods on `DoClient`
- **`bp-snapshot` subcommand** — create a named snapshot of a BytePlus ECS instance's system disk via StorageEBS API (`--instance-id` + `--snapshot-name`)
- **`bp-restore` subcommand** — restore a new BytePlus ECS instance from a snapshot: creates a custom image from the snapshot, then launches a new instance with SSH key, deploy record, and inline EIP
- **`ls-snapshot` subcommand** — create a snapshot of an AWS Lightsail instance (`--instance-name` + `--snapshot-name`)
- **`ls-restore` subcommand** — restore a new Lightsail instance from a snapshot (direct snapshot-to-instance, no intermediate image step)
- **Lightsail snapshot API** — `create_instance_snapshot()`, `get_snapshot()`, `list_snapshots()`, `wait_for_snapshot()`, `create_instance_from_snapshot()` methods on `LightsailCliProvider`
- **BytePlus StorageEBS API** — `describe_system_volume()`, `create_ebs_snapshot()`, `describe_snapshots()`, `wait_for_snapshot()` methods on `BytePlusClient`
- **BytePlus image management** — `create_image()`, `wait_for_image()`, `create_instance_from_image()` methods on `BytePlusClient`

### Fixed
- **BytePlus EIP orphan cleanup** — destroy command now lists and releases unbound EIPs (`release_unbound_eips()`) after instance termination, preventing orphaned EIP charges
- **BytePlus VPC cleanup reliability** — waits for instance deletion to fully propagate (up to 60s), retries subnet/VPC deletion up to 3 times
- **BytePlus DNS resolution** — configures public DNS fallback (8.8.8.8, 1.1.1.1) on BytePlus instances to fix Telegram and external API resolution failures
- **Sandbox mode disabled by default** — explicitly sets `sandbox.mode=off` when `--enable-sandbox` is not passed, preventing Docker-not-found errors

### Changed
- **BytePlus EIP: pay-by-traffic billing** — switched from `BillingType: 2` (pay-by-bandwidth at 10 Mbps) to `PostPaidByTraffic` (pay-by-traffic at 5 Mbps), significantly reducing costs for low-traffic instances
- **BytePlus EIP: inline creation** — EIP is now created as part of `RunInstances` via the `EipAddress` parameter with `ReleaseWithInstance: true`, eliminating the separate `AllocateEipAddress` + `AssociateEipAddress` calls and ensuring automatic EIP cleanup on instance termination
- **BytePlus `wait_for_running` simplified** — no longer allocates/associates EIP separately; polls until instance is RUNNING with public IP already assigned
- **BytePlus spot instances** — new `--spot` CLI flag enables `SpotAsPriceGo` strategy on BytePlus deploys for up to ~80% compute cost savings (instance may be reclaimed with 5 min warning)

## v0.23.0

### Fixed
- Committed `npm/clawmacdo/README.md` to the repo so npmjs.com always displays the correct, up-to-date README regardless of commit order relative to tagging

## v0.22.0

### Changed
- Minor version bump; README updated to reflect v0.21.0 additions (destroy command, skills-data-api service, Playwright e2e suite, project structure docs)

## v0.21.0

### Changed
- Bumped minor version following addition of `skill`, `do-restore`, `tailscale-funnel`, `destroy` commands, BytePlus and DigitalOcean cloud provider support, Playwright e2e test suite, and skills-data-api service

## v0.20.0

### Added
- **`do-restore` subcommand** — restore a DigitalOcean droplet from a snapshot by name (`--do-token` + `--snapshot-name`), with standard `openclaw-{id}` naming, SSH key generation, and deploy record saved to both JSON and SQLite (visible in web UI Deployments tab)
- **DigitalOcean snapshot API** — `list_snapshots()` and `create_droplet_from_snapshot()` methods on `DoClient`

## v0.19.0

### Added
- **One-click Funnel access** — "Open" button in Deployments tab opens the Funnel URL with gateway token pre-injected via `auth.html` (external JS to satisfy CSP `script-src 'self'`)
- **Auto-disable device pairing for Funnel** — Funnel setup now sets `controlUi.dangerouslyDisableDeviceAuth: true` in `openclaw.json`, eliminating the mandatory device pairing step for browser connections through Tailscale Funnel

### Fixed
- **Funnel "pairing required" blocker** — browser connections via Tailscale Funnel no longer get stuck at the device pairing screen
- **CSP inline script violation** — moved auth page JavaScript to external `assets/auth.js` file served from the same origin

## v0.18.0

### Added
- **`tailscale-funnel` subcommand** — full Tailscale Funnel setup: install Tailscale, connect with auth key, enable Funnel, configure `openclaw.json` (`controlUi.allowedOrigins` + `trustedProxies`), auto-approve pending devices, and print public webchat URL with auth token
- **`funnel-on` / `funnel-off` subcommands** — toggle Tailscale Funnel on/off via SSH
- **`device-approve` subcommand** — auto-approve all pending OpenClaw webchat device pairing requests
- **`skill-upload` subcommand** — upload a local SKILL.md to the Railway skills API and SCP it to the OpenClaw instance (with backup)
- **`skill-download` subcommand** — download a customer SKILL.md from the Railway skills API
- **`skill-push` subcommand** — push a SKILL.md from the Railway API directly to the instance via SCP (with backup)
- **User-skills API endpoints** — `POST/GET/DELETE /api/user-skills/:deploymentId` and `/info` for per-deployment custom SKILL.md management, protected with `x-api-key` (`USER_SKILLS_API_KEY`)
- **Web UI Funnel toggle** — Deployments tab now has a Funnel column with on/off toggle button that shows the public URL
- **Web UI logout** — `/logout` endpoint and logout link in the header

### Fixed
- **Deployments tab 401 after PIN login** — `api_key_middleware` now also accepts a valid PIN session cookie, so browser fetch calls to `/api/*` work after logging in
- **PIN login error not showing** — removed HTML5 validation (`required`, `pattern`) so invalid PINs reach the server and display the error message

## v0.17.0

### Security
- **CRIT-01: Web UI authentication** — API key middleware for all `/api/*` endpoints (`CLAWMACDO_API_KEY`), 6-digit PIN login for web pages (`CLAWMACDO_PIN`), CORS middleware restricting origins, per-IP rate limiting (60 req/min), and localhost-only binding by default (`CLAWMACDO_BIND`)
- All 4 CRITICAL security findings from the security audit are now resolved

### Changed
- Server now binds to `127.0.0.1` by default instead of `0.0.0.0` — set `CLAWMACDO_BIND=0.0.0.0` for remote access

## v0.16.0

### Added
- **BytePlus destroy cleanup** — automatically release EIP and delete VPC/subnet/security-group when destroying BytePlus instances
- **BytePlus deploy form improvements** — auto-default primary AI model to "byteplus" when BytePlus provider selected; "Generate" button for ARK API key with endpoint selection
- **ARK API endpoints** — `POST /api/ark/endpoints` and `POST /api/ark/api-key` for ARK key generation from the web UI
- **Playwright E2E test suite** — 30 CSV-driven test scenarios covering all 5 cloud providers (DigitalOcean, Tencent, AWS Lightsail, Azure, BytePlus) with all model/failover/messaging permutations

## v0.15.0

### Fixed
- **Windows build failures** — dependencies were incorrectly scoped under `[target.'cfg(unix)'.dependencies]`, making reqwest, rusqlite, serde, axum, and all internal workspace crates invisible on Windows
- **Missing `sync` feature on tokio** — `deploy.rs` and `serve.rs` use `tokio::sync::{mpsc, RwLock}` which requires the `sync` feature
- **Missing `digitalocean` feature** — `deploy.rs` unconditionally imports `clawmacdo_cloud::digitalocean`, now properly gated with default feature

## v0.14.0

### Added
- **`ark-api-key` subcommand** — generate temporary BytePlus ARK API keys from access/secret credentials with HMAC-SHA256 signing
- **`ark-api-key --list`** — list available ARK inference endpoints directly from the CLI
- **`ark-chat` subcommand** — send chat completion prompts to BytePlus ARK models (OpenAI-compatible API)
- **`telegram-setup` subcommand** — configure Telegram bot token on a deployed instance via SSH
- **`telegram-pair` subcommand** — approve Telegram pairing code to activate chat
- **Web UI destroy with cloud cleanup** — Deployments tab "Destroy" button now destroys the cloud instance and deletes the local record, with provider-specific credential prompts
- **Comprehensive usage guide** — `docs/clawmacdo_usage.md` with all CLI examples, curl commands, and sample responses

### Fixed
- **Web UI destroy handles missing cloud instances** — if the instance was already deleted from the cloud, the local record is still cleaned up (previously left orphaned)
- **Detach mode improvements** — proper `setsid()` session detachment, stdout/stderr logging to deploy log file
- **Empty `track` query** — returns most recent deployment instead of matching stale records with empty hostname
- **`/root/.openclaw/workspace` permission error** — automatic path correction from `/root/` to `/home/openclaw/` during provisioning and backup restore

## v0.13.0

### Added
- **BytePlus ARK** as default AI model provider when `--provider=byteplus` is selected
- `BYTEPLUS_API_KEY` env var written to `.env` during provisioning
- BytePlus ARK model config (`openclaw.json`) auto-configured with ARK API endpoint
- `destroy` CLI subcommand for all providers (was previously missing from CLI)
- BytePlus `destroy` handler with `--yes` flag for non-interactive cleanup
- EIP (Elastic IP) allocation and association for BytePlus VPC instances

### Fixed
- **SSH heredoc syntax error**: `{ cmd ; } 2>&1` wrapping broke bash when commands contained heredocs; changed to `{ cmd\n} 2>&1`
- **SSH connection drop at Step 10**: `ufw reload` killed active SSH sessions; now conditional (only when Docker installed) and runs detached via `nohup`
- **Docker provision crash on BytePlus**: `docker.io` not available on BytePlus images; Docker configuration now skips gracefully when Docker is not installed
- **Security group rule conflicts**: BytePlus returns `InvalidSecurityRule.Conflict` (409) for existing rules; now handled idempotently
- **Missing public IP on BytePlus**: VPC instances get no public IP by default; EIP is now auto-allocated and associated
- Removed debug `eprintln!` logging from BytePlus client
- Dead `stderr_out` code cleaned up in `ssh::exec()`

## v0.12.2

### Fixed
- BytePlus API endpoint corrected to `open.byteplusapi.com` (was using non-existent `open.ecs.byteplusapi.com`)

## v0.12.1

### Fixed
- README "What's New" section now reflects v0.12.x (was stuck on v0.11.0)

## v0.12.0

### Added
- **BytePlus Cloud** as 5th cloud provider (`--provider=byteplus` or `bp`)
- BytePlus ECS client with HMAC-SHA256 signing (similar to AWS SigV4)
- Auto-provisioning of VPC, subnet, and security group on BytePlus
- Web UI dropdown, credential fields, region/size selectors for BytePlus
- `byteplus` feature flag in `clawmacdo-cloud` and `clawmacdo-cli`

## v0.11.0

### Added
- Preflight CLI checks at startup — auto-installs Azure CLI and AWS CLI if missing
- Full-width web UI layout (`max-w-screen-2xl`) with compact hero section
- Version badge in web UI header

### Changed
- Web UI mascot moved inline alongside provider description for a professional look
- Header tagline updated to "Deploy OpenClaw to the Cloud"

## v0.10.0

### Fixed
- Make `serde_json` and `rusqlite` non-optional in CLI crate (fixes build without `web-ui` feature)

### Changed
- `web-ui` feature flag now only gates `axum` and `tokio-stream` dependencies

## v0.9.1

### Fixed
- npm package now includes README.md on npmjs.com (copies repo README into package before publish)

## v0.9.0

### Added
- SQLite `deploy_steps` table to persist step-level deploy progress (WAL mode enabled)
- `clawmacdo track <query>` CLI command — query by deploy ID, hostname, or IP
- `--follow` mode: live-polling display that refreshes until deployment completes
- `--json` mode: NDJSON output for programmatic consumption
- Clap-based CLI with `track` and `serve` subcommands (replaces placeholder main)
- Step recording helpers (`record_step_start`, `record_step_complete`, `record_step_failed`, `record_step_skipped`)
- All 16 deploy steps instrumented with DB writes across DigitalOcean, Tencent, Lightsail, and Azure providers
- Step callback system (`on_step`/`on_step_done`) in `ProvisionOpts` for steps 9-14
- `get_deployment_by_id` and `find_deployment_by_query` lookup functions in clawmacdo-db

### Changed
- Web UI deploys now automatically persist step progress via shared DB handle

## v0.8.0

### Added
- npm distribution packaging for cross-platform binary (`npm install -g clawmacdo`)
- Platform-specific npm packages: `@clawmacdo/darwin-arm64`, `@clawmacdo/linux-x64`, `@clawmacdo/win32-x64`
- TypeScript type definitions for programmatic binary path resolution
- GitHub Actions workflow for automated npm publishing on release
- Build and publish scripts (`scripts/npm-package.sh`, `scripts/npm-publish.sh`)
- x-api-key middleware using verify_apikey and KEY_DB for API authentication
- `verify_apikey` helper for API key verification
- `gen_apikey.sh` script to create one-time API keys and store hashed values in SQLite
- Security flaw evaluation report with functionality impact assessment

### Fixed
- Correct ROOT path resolution to repo root in API
- Resolve verify script path and use project root cwd
- Make openclaw_config_scan resource-friendly by targeting config files

### Changed
- Update .gitignore and .env.example with comprehensive entries

## v0.7.0

### Added
- Microsoft Azure Compute VM as fourth cloud provider (service principal auth)
- Azure CLI integration (`az` commands) — feature-gated with `#[cfg(feature = "azure")]`
- Azure credential fields: Tenant ID, Subscription ID, Client ID, Client Secret
- Resource group-based lifecycle: `clawmacdo-<id>` resource group per deploy, full cleanup on destroy
- Azure regions (13 locations, default: southeastasia) and VM SKUs (6 sizes, default: Standard_B2s)
- Azure option in web UI provider dropdown with dynamic credential fields, regions, and sizes
- Parameterized cloud-init: `generate_for_user()` / `generate_shell_for_user()` support `azureuser` admin user
- `Azure` variant in `CloudProviderType` enum and `Azure(String)` error variant in `AppError`
- `resource_group` field on `DeployRecord` (with `#[serde(default)]` for backward compat)

### Changed
- `azure` feature enabled by default in `clawmacdo-cli`
- `cloud_init::generate()` now delegates to `generate_for_user("ubuntu")` (no behavior change for existing providers)

## v0.6.0

### Added
- Configurable primary AI model: any of Anthropic, OpenAI, or Gemini can be selected as the primary model
- User-ordered failover chain: remaining models available as optional failovers in chosen order
- New CLI flags: `--primary-model`, `--failover-1`, `--failover-2` for Deploy and Migrate commands
- Dynamic model selector UI in web dashboard with cascading dropdowns
- Server-side validation ensuring primary model's API key is present

### Changed
- Replaced hardcoded `build_failover_setup_cmd` with flexible `build_model_setup_cmd` that supports any primary/failover combination
- Model setup command now always runs (sets primary even with zero failovers)
- Anthropic API key no longer hardcoded as required; requirement follows primary model selection

## v0.5.0

### Added
- AWS Lightsail provider fully integrated into the web UI
- Provider dropdown now includes DigitalOcean, Tencent Cloud, and AWS Lightsail
- AWS credential fields (Access Key ID, Secret Access Key) in the deploy form
- Lightsail-specific regions (12 AWS regions) and instance sizes (micro through xlarge)
- Lightsail variant added to CloudProviderType enum

### Fixed
- Lightsail deploy path now uses the shared provisioning flow (matching DO and Tencent)
- Resolved 18 compilation errors from incomplete Lightsail integration
- Fixed missing AWS fields in migrate and serve DeployParams constructors
- Made provision submodules public for cross-crate access

## v0.4.0

### Changed
- Resolve all clippy warnings across 15 source files (42 fixes)
- Apply `cargo fmt` formatting to entire codebase
- Inline format arguments, remove unused imports, suppress dead_code on provider abstractions
- Replace useless `format!`/`vec!` macros with direct literals
- Add CLAUDE.md with versioning strategy and Rust project practices

## v0.3.0

### Added
- Tencent Cloud provider support (deploy, destroy, status)
- Web UI with instance type selection for both providers
- `--yes`/`--force` flag on destroy command to skip TTY confirmation













