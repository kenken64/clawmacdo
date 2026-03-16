# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs`: CLI entry; wires subcommands and async runtime.
- `src/commands/`: user-facing commands (`backup`, `deploy`, `migrate`, `destroy`, `status`, `list_backups`, `serve`, `whatsapp-repair`, `docker-fix`).
- `src/provision/`: droplet setup steps (`user`, `firewall`, `docker`, `nodejs`, `openclaw`, `tailscale`).
- `src/{digitalocean,ssh,cloud_init,ui,progress,config,error}.rs`: service clients, helpers, and types.
- `assets/`: static files for UI or docs.

## Build, Test, and Development Commands
- Build release: `cargo build --release` (outputs to `target/release/`).
- Run CLI: `cargo run -- <command>` (e.g., `cargo run -- status --do-token $DO_TOKEN`).
- Run web UI: `cargo run -- serve --port 3456`.
- Format: `cargo fmt --all`.
- Lint: `cargo clippy -- -D warnings`.
- Cross-compile (CI mirrors): tags `v*` trigger multi-target builds via GitHub Actions.

## Coding Style & Naming Conventions
- Rust 2021, 4-space indent, default `rustfmt` style.
- Modules/files `snake_case`; types/traits `PascalCase`; functions/locals `snake_case`.
- Error handling: prefer `anyhow::Result` at orchestration boundaries; use `thiserror` for domain errors (`src/error.rs`).
- CLI args via `clap` derive; keep flags/env names consistent with README examples.

## Testing Guidelines
- No formal tests yet. Prefer adding integration tests under `tests/` (invoke subcommands), and unit tests with `#[cfg(test)]` inside modules.
- Test names: describe behavior (e.g., `deploy_writes_env_when_keys_supplied`).
- Run: `cargo test` (ensure it passes on Linux/macOS and Windows).

## Commit & Pull Request Guidelines
- Commit style: prefer Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `release:`). Keep subject ≤72 chars.
- PRs: include a concise description, reproduction/validation steps, and screenshots or terminal output for `serve` UI or CLI flows.
- Releases: push a tag like `v0.2.3` to trigger the release workflow; pushes to `main` update the rolling `latest` changelog.

## Security & Configuration Tips
- Never commit secrets (`DO_TOKEN`, API keys). Support both flags and env vars (e.g., `DO_TOKEN`, `ANTHROPIC_API_KEY`).
- Windows build: requires MSVC + Windows SDK. The repo includes `.cargo/config.toml` with example paths—adjust locally if your toolchain differs.
- Linux build deps: `libssl-dev`, `pkg-config` (CI installs them).

## Contributor Notes (Agents & Humans)
- Keep changes minimal and focused; follow existing module boundaries (`commands/*`, `provision/*`).
- Update `README.md` when user-facing flags/flows change.
- Run `cargo fmt` and `cargo clippy` before opening a PR.
