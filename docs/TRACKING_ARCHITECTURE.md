# Provision Tracking Architecture

## Overview

Real-time deployment progress tracking via SQLite persistence and a CLI `track` command. Designed for multi-user concurrency where a Next.js frontend spawns short-lived CLI processes to stream progress.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│  Next.js App (consumer)                                  │
│                                                          │
│  POST /api/deploy                                        │
│    → spawn "clawmacdo deploy --json --background"        │
│    → CLI writes steps to SQLite, returns deploy_id       │
│    → respond { deployId: "abc-123" }                     │
│                                                          │
│  GET /api/deploy/:id/progress                            │
│    → spawn "clawmacdo track <id> --follow --json"        │
│    → pipe stdout as SSE to browser                       │
│    → client disconnects? process dies. reconnect?        │
│      new track process picks up from current state       │
│                                                          │
│  GET /api/deploys                                        │
│    → spawn "clawmacdo status --json"                     │
│    → return all deployments with current step progress   │
└──────────────────────────────────────────────────────────┘
         │                              │
    (detached)                    (short-lived)
         ▼                              ▼
┌─────────────────┐          ┌──────────────────┐
│ clawmacdo deploy│──write──▶│   SQLite (WAL)   │◀──read── clawmacdo track
│  (runs 5-10min) │          │  deploy_steps    │         (runs until done
│                 │          │  deployments     │          or client drops)
└─────────────────┘          └──────────────────┘
```

## Data Model

### New table: `deploy_steps`

```sql
CREATE TABLE IF NOT EXISTS deploy_steps (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    deploy_id    TEXT NOT NULL REFERENCES deployments(id),
    step_number  INTEGER NOT NULL,
    total_steps  INTEGER NOT NULL DEFAULT 16,
    label        TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'running',
    started_at   TEXT NOT NULL,
    completed_at TEXT,
    error_msg    TEXT,
    UNIQUE(deploy_id, step_number)
);
```

Step status values: `running`, `completed`, `failed`, `skipped`

### SQLite WAL mode

Enabled at connection init for concurrent read/write:

```sql
PRAGMA journal_mode=WAL;
```

Allows multiple `track` readers while one `deploy` writer records steps.

## CLI Commands

### `clawmacdo deploy --json --background`

Detaches the deploy process and immediately returns a deploy ID:

```json
{"event":"deploy_started","deploy_id":"abc-123"}
```

The detached process writes each step to the `deploy_steps` table as it progresses.

### `clawmacdo track <query> [--follow] [--json]`

Tracks deployment progress by deploy ID, hostname, or IP address.

**Arguments:**
- `<query>` — deploy UUID, hostname, or IP address
- `--follow` / `-f` — poll DB every 1s and live-update (exit when deploy finishes)
- `--json` — output NDJSON for machine consumption

**Human-readable output:**

```
Deployment: abc12345-xxxx-yyyy
Provider:   digitalocean  |  IP: 1.2.3.4  |  Status: running
Created:    2026-03-14 10:30:00 UTC

Progress (12/16):
  ✓  Step  1/16  Resolving parameters
  ✓  Step  2/16  Generating SSH key pair
  ✓  Step  3/16  Uploading SSH public key
  ✓  Step  4/16  Creating droplet
  ✓  Step  5/16  Waiting for droplet to become active
  ✓  Step  6/16  SSH ready
  ✓  Step  7/16  Cloud-init complete
  —  Step  8/16  No backup to restore (skipped)
  ✓  Step  9/16  Creating openclaw user
  ✓  Step 10/16  Hardening firewall
  ✓  Step 11/16  Configuring Docker
  ▸  Step 12/16  Setting up Node.js/pnpm...
     Step 13/16  Installing OpenClaw
     Step 14/16  Tailscale
     Step 15/16  Starting gateway
     Step 16/16  Saving deploy record
```

**JSON output (NDJSON):**

```jsonl
{"event":"step_start","step":1,"total":16,"label":"Resolving parameters","ts":"2026-03-14T10:30:00Z"}
{"event":"step_complete","step":1,"total":16,"label":"Resolving parameters","ts":"2026-03-14T10:30:01Z"}
{"event":"step_start","step":2,"total":16,"label":"Generating SSH key pair","ts":"2026-03-14T10:30:01Z"}
{"event":"step_complete","step":2,"total":16,"label":"Generating SSH key pair","ts":"2026-03-14T10:30:02Z"}
{"event":"step_failed","step":6,"total":16,"label":"SSH ready","error":"Connection refused","ts":"..."}
{"event":"deploy_complete","deploy_id":"abc-123","ip":"1.2.3.4","hostname":"openclaw-abc","ts":"..."}
```

### `clawmacdo status --json`

Lists all deployments with their current step progress:

```jsonl
{"id":"abc-123","provider":"digitalocean","hostname":"openclaw-abc","ip":"1.2.3.4","status":"running","progress":{"completed":12,"total":16},"created_at":"..."}
{"id":"def-456","provider":"lightsail","hostname":"openclaw-def","ip":"5.6.7.8","status":"completed","progress":{"completed":16,"total":16},"created_at":"..."}
```

## Concurrency Model

| Concern | How it's handled |
|---------|-----------------|
| Multiple simultaneous deploys | Each deploy is a separate process writing to its own `deploy_id` rows |
| Multiple users tracking same deploy | Multiple `track` processes read the same rows (WAL allows concurrent reads) |
| Browser disconnect / reconnect | `track` process dies on disconnect; new one resumes from current DB state |
| Next.js restart | Detached deploy processes continue; new `track` picks up from DB |
| Serverless (Vercel) | Short-lived `track` requests; deploy runs on a separate worker |
| Historical lookups | Steps persist in DB; `track <old-id>` works after deploy finishes |

## Implementation Plan

### Phase 1: Database schema (clawmacdo-db)
- Add `deploy_steps` table creation to `init_db()`
- Enable WAL mode
- Add functions: `insert_deploy_step`, `complete_deploy_step`, `fail_deploy_step`, `get_deploy_steps`, `get_deployment_by_id`, `find_deployment_by_query`
- Add `DeployStepRow` struct

### Phase 2: Track command (clawmacdo-cli)
- Create `commands/track.rs`
- Implement one-shot and `--follow` modes
- Implement `--json` output (NDJSON)
- Human-readable rendering with colored status indicators
- Register in `commands/mod.rs`

### Phase 3: Instrument deploy flow (clawmacdo-cli)
- Add `--json` and `--background` flags to `DeployParams`
- Add DB handle to `DeployParams`
- Insert step start/complete calls at each of the 16 step boundaries
- Support detached process mode for `--background`

### Phase 4: Wire CLI subcommands (clawmacdo-cli)
- Transform `main.rs` from placeholder to clap-based CLI
- Wire up: `deploy`, `track`, `status`, `destroy`, `backup`, `list-backups`, `migrate`, `serve`

### Phase 5: Update serve.rs
- Pass DB handle through deploy params for web-initiated deploys
- Steps are recorded in DB for both CLI and web deploys

### Files to modify/create

| File | Action |
|------|--------|
| `crates/clawmacdo-db/src/db.rs` | Modify — add `deploy_steps` table, WAL mode, step CRUD |
| `crates/clawmacdo-cli/src/commands/track.rs` | **Create** — track command |
| `crates/clawmacdo-cli/src/commands/deploy.rs` | Modify — add `--json`, `--background`, instrument steps |
| `crates/clawmacdo-cli/src/commands/mod.rs` | Modify — register `track` module |
| `crates/clawmacdo-cli/src/main.rs` | Modify — clap subcommand routing |
| `crates/clawmacdo-cli/src/commands/serve.rs` | Modify — pass DB handle to deploy |
