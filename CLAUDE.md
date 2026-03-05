## Versioning Strategy

Use Semantic Versioning (SemVer: MAJOR.MINOR.PATCH). When asked to bump the version:

### Bump PATCH when:
- Bug fixes with no API or behavior changes
- Internal refactoring, dependency updates, security patches

### Bump MINOR when:
- New features or endpoints added (backward-compatible)
- New optional parameters or response fields added
- Functionality deprecated but not removed

### Bump MAJOR when:
- Breaking changes to public API (removed/renamed endpoints, changed signatures)
- Auth mechanism changes
- Any change that requires consumers to update their code

### How to apply:
1. Determine the bump type based on the git diff / changes made
2. Update `package.json` (or `Cargo.toml` / `pyproject.toml` etc.) version field
3. Update `CHANGELOG.md` with a summary under the new version heading
4. Suggest a git tag command: `git tag v<new_version>`

## Rust Project Good Practices

### Code Quality
- Keep modules small and focused; prefer clear boundaries (`commands/*`, `provision/*`, service clients).
- Follow idiomatic naming: modules/functions in `snake_case`, types/traits in `PascalCase`.
- Run formatting and lints before commit:
  - `cargo fmt --all`
  - `cargo clippy -- -D warnings`
- Avoid `unwrap()`/`expect()` in production paths; propagate errors with context.

### Error Handling
- Use `anyhow::Result` at orchestration boundaries (CLI command handlers).
- Use typed domain errors with `thiserror` for reusable/library-level errors.
- Add actionable error messages (what failed, where, and next recovery step).

### Testing
- Add unit tests in modules (`#[cfg(test)]`) for parsing, validation, and helpers.
- Add integration tests under `tests/` for CLI behavior and command flows.
- Name tests by behavior (example: `deploy_writes_env_when_keys_supplied`).
- Run `cargo test` locally before release tagging.

### Dependency and Security Hygiene
- Keep dependencies minimal and pinned via `Cargo.lock`.
- Avoid committing secrets or tokens; use env vars and CLI flags.
- Review dependency updates for security and breaking changes.

### Release Hygiene
- Keep `Cargo.toml` and `Cargo.lock` package version in sync for releases.
- Ensure `README.md` and `CHANGELOG.md` reflect user-facing changes.
- Validate release flow on CI before publishing binaries/tags.
