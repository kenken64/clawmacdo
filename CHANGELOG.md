# Changelog

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
