# Hermes native bundles

The pinned versions, source commit, installer checksums and Ubuntu runtime libraries live in
`bundle.json`. The `Hermes native bundle` workflow builds on clean Ubuntu 24.04 amd64 and arm64
VMs, then installs each artifact on another VM. Runtime tests use a network namespace without
external connectivity, so missing dependencies cannot be downloaded silently.

## Current published bundle

Clawmacdo **v0.99.0** automatically uses
[`hermes-bundle-v0.22.5-1`](https://github.com/kenken64/clawmacdo/releases/tag/hermes-bundle-v0.22.5-1)
on Ubuntu 24.04 amd64/arm64. It includes Hermes **0.22.5** at commit
`bf0203a794a07aac3ff764972ab9087baa4e4f4e`, OpenCode **2.1.3**, and cloudflared **2026.10.3**.
Both architectures passed the [build and fresh-VM verification workflow](https://github.com/kenken64/clawmacdo/actions/runs/40023821213).

Each architecture has a `hermes-ubuntu-24.04-<arch>.tar.gz` archive, a `.tar.gz.json` manifest,
and a `.tar.gz.sha256` checksum. The CLI downloads the matching archive automatically during
`hermes-provision`; users do not need to download or publish an AWS image.

## Local bundles (macOS and Windows)

`hermes-local-install` installs `hermes-macos-arm64.tar.gz` (macOS Apple Silicon) or
`hermes-windows-x64.tar.gz` (Windows x64) from the same release, so nothing compiles on the user's
computer. `build-local.sh` and `build-local.ps1` run the pinned upstream installer, with a verified
checksum, on clean `macos-14` and `windows-latest` runners. `package.py --platform` then packages
only the Hermes home's software folders: `hermes-agent` (checkout, built dashboard and terminal UI,
Node dependencies), `tools` (pinned Python, Node, npm and uv) and `installs` (the Python
environment). Launchers, skills, configuration, `.env`, sessions and logs are never included.

Upstream binds an install to its location in a few places. Its per-install state folder is
`installs/<sha256(checkout path)[:16]>`, and some text files hold absolute paths. The packager scans
every packaged file for the build's home paths. It lists the text files in the manifest's
`relocate` field and writes a report of all matches as the `report-<platform>` workflow artifact.
Binary matches, such as native debug strings, are reported but not rewritten. On install, the CLI:

1. checks the manifest's release, platform and Hermes commit against its own pins;
2. checks the archive's size and SHA-256;
3. extracts only paths inside the three folders (links must stay inside them);
4. renames the state folder for the new checkout path;
5. rewrites the listed files in one pass (on Windows this covers backslash, forward-slash and
   JSON-escaped spellings, ignoring case);
6. runs upstream's `hermes_cli/source_completion.py` with the bundled Python to publish launchers
   for the new location.

The `verify-local` job installs each archive with the real CLI into a different home folder,
with `CC=false`, `CXX=false`, `npm_config_offline=true` and `UV_OFFLINE=1`, so any rebuild or
package download fails. It then checks that no text file still names the build location. It runs
`hermes --version` and `hermes pm doctor`, serves the compiled dashboard, and uninstalls.

A missing local asset stops the install with nothing changed. The CLI does not fall back to
compiling unless the user passes `--from-source`.

## Contents and installation

This is an Ubuntu-specific bundle installed at `/home/ubuntu`, preserving Hermes's native
launcher and environment paths. The package includes an explicit list of software directories;
it does not copy the whole home directory, credentials, configuration, SSH keys, sessions or logs.
Hermes retains its source checkout and native update mechanism. OpenCode and cloudflared are
stored as bundled binaries and activated only when their provisioning options are enabled.

## Updating a bundle

1. Update the pinned versions/checksums and increment `release` in `bundle.json`.
2. Run the workflow and require both fresh-VM verification jobs to pass.
3. Push the matching `hermes-bundle-v…` tag. The workflow rebuilds, verifies and publishes the
   archives, manifests and checksums as a separate release without changing the CLI's latest release.
4. Release the CLI containing the matching bundle configuration.

The workflow runs on manual dispatch and `hermes-bundle-v*` tags. A manual run builds and verifies;
only a matching bundle tag publishes assets. CLI `v*` releases and npm package publishing remain
separate from runtime bundle releases.

## Integrity and fallback behavior

The client accepts only the matching platform, release and source commit. It verifies the complete
archive before extracting it as the `ubuntu` user and refuses existing software or archive paths
outside the allowed directories. A missing asset (HTTP 404) or unsupported platform enables source
fallback; integrity errors and other download failures do not. The source installer runs after OS
prerequisites are installed to keep all package-manager operations serialized.

Starting with CLI v0.98.0, foreground OS setup refreshes apt indexes and installs required runtime
libraries. After step 8 saves the deployment record, SSH queues
`clawmacdo-hermes-os-upgrade.service` with `systemctl start --no-block`. Full OS upgrades continue
independently while Hermes is usable; v0.97.0 upgraded packages during foreground setup.
The worker reports status in `/var/lib/clawmacdo/hermes-os-upgrade/status` and logs to the systemd
journal. Launch failures are reported separately from deployment success, and dry runs never queue
the job. The worker preserves existing package configuration, uses needrestart's list-only mode,
and reports a required reboot without performing one automatically.

The expected foreground duration is the slower of required OS setup and bundle download/extraction,
plus service configuration and readiness checks. Compare measured timings on the same Lightsail
plan and region before claiming a speed improvement; CI runner times are not Lightsail provisioning
benchmarks.

## Local checks

```sh
python3 tests/unit/test_hermes_bundle.py
python3 tests/unit/test_hermes_cloudflare.py
cargo test --workspace
```
