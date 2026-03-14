# Changelog

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
