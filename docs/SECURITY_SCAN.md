# Security Scan CLI

This document describes the security-scan feature added to clawmacdo.

- Usage: `clawmacdo-cli scan --target=ubuntu --out=/tmp/out.json` or use `FORMAT=text` to produce a text summary.
- Scripts: scripts/run_all_scans.ps1, scripts/ubuntu_scan.ps1, scripts/macos_scan.ps1, scripts/windows_scan.ps1
- CI: .github/workflows/security-scan.yml runs unit & integration tests.

See tests and scripts for details.
