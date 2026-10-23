# Hermes native bundles

The pinned versions, source commit, installer checksums and Ubuntu runtime libraries live in
`bundle.json`. The `Hermes native bundle` workflow builds on clean Ubuntu 24.04 amd64 and arm64
VMs, then installs each artifact on another VM. Runtime tests use a network namespace without
external connectivity, so missing dependencies cannot be downloaded silently.

This is an Ubuntu-specific bundle installed at `/home/ubuntu`, preserving Hermes's native
launcher and environment paths. The package includes an explicit list of software directories;
it does not copy the whole home directory, credentials, configuration, SSH keys, sessions or logs.
Hermes retains its source checkout and native update mechanism. OpenCode and cloudflared are
stored as bundled binaries and activated only when their provisioning options are enabled.

To update a bundle:

1. Update the pinned versions/checksums and increment `release` in `bundle.json`.
2. Run the workflow and require both fresh-VM verification jobs to pass.
3. Push the matching `hermes-bundle-v…` tag. The workflow rebuilds, verifies and publishes the
   archives, manifests and checksums as a separate release without changing the CLI's latest release.
4. Release the CLI containing the matching bundle configuration.

The client accepts only the matching platform, release and source commit. It verifies the complete
archive before extracting it as the `ubuntu` user and refuses existing software or archive paths
outside the allowed directories. A missing asset (HTTP 404) or unsupported platform enables source
fallback; integrity errors and other download failures do not. The source installer runs after OS
updates to keep all package-manager operations serialized.

OS updates are still enabled. The expected total duration is the slower of OS setup and bundle
download/extraction, plus service configuration and readiness checks. Compare measured timings on
the same Lightsail plan and region before claiming a speed improvement; CI runner times are not
Lightsail provisioning benchmarks.

Local checks:

```sh
python3 tests/unit/test_hermes_bundle.py
python3 tests/unit/test_hermes_cloudflare.py
cargo test --workspace
```
