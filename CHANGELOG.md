# Changelog

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
