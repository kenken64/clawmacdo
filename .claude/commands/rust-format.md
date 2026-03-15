---
description: Format, lint, and sanity-check the Rust project using standard tooling.
allowed-tools: Bash
---

Run the Rust quality pipeline in this order:

1. `cargo fmt --all`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test`

Then report:
- What changed (files touched by formatting)
- Whether clippy/test passed
- Any follow-up fixes required

If `cargo fmt` changes files, keep those edits.
If clippy fails, fix issues when reasonable and re-run clippy.
