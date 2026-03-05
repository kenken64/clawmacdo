# OpenClaw Deployment Architecture Research

## Question: Should we containerize the entire OpenClaw gateway?

**Date:** 2026-03-05
**Author:** Simon (AI) + Kenneth Phang
**Status:** Research complete — recommendation: stay on host

---

## Current Architecture

```
┌──────────────────────────────────────────────┐
│                   HOST                        │
│                                               │
│  ┌─────────────────────────────────────────┐  │
│  │  OpenClaw Gateway (systemd service)     │  │
│  │  - Node.js process                      │  │
│  │  - WhatsApp (Baileys) connection        │  │
│  │  - Telegram bot                         │  │
│  │  - API keys in ~/.openclaw/.env         │  │
│  │  - Workspace at ~/.openclaw/workspace   │  │
│  └──────────────┬──────────────────────────┘  │
│                 │                              │
│                 │ Docker socket                │
│                 ▼                              │
│  ┌─────────────────────────────────────────┐  │
│  │  Sandbox Containers (per-session)       │  │
│  │  - Tool execution (exec, read, write)   │  │
│  │  - Filesystem isolation                 │  │
│  │  - No network (default)                 │  │
│  └─────────────────────────────────────────┘  │
└──────────────────────────────────────────────┘
```

- **Gateway** runs directly on the host as a systemd user service
- **Sandbox** containers are spawned by the gateway via Docker for tool isolation
- **What's NOT sandboxed:** Gateway process, messaging connections, API keys, config

---

## Three Options Evaluated

### Option A: Host-based (Current)

Gateway on host, sandbox in Docker containers.

| Aspect | Assessment |
|--------|-----------|
| Security | Gateway has full host access — prompt injection risk |
| WhatsApp | Native pairing, seamless QR scan |
| Performance | Best — no container overhead |
| Debugging | Easiest — direct SSH, journalctl, systemctl |
| clawmacdo | Fully supported (16-step SSH provisioning) |
| Complexity | Lowest |

### Option B: Full Docker

Everything in Docker — gateway + tools + sandbox.

| Aspect | Assessment |
|--------|-----------|
| Security | Gateway isolated, but needs Docker socket (negates benefit) |
| WhatsApp | Pairing problematic — session persistence via volumes |
| Performance | Slight overhead (NAT, overlay2 filesystem) |
| Debugging | Harder — docker exec, docker logs |
| clawmacdo | Major rewrite needed |
| Complexity | Highest |

### Option C: Hybrid Docker

Gateway in Docker, sandbox as sibling containers via Docker socket.

| Aspect | Assessment |
|--------|-----------|
| Security | Docker socket paradox (see below) |
| WhatsApp | Volume-mounted sessions, still fragile |
| Performance | Medium overhead |
| Debugging | Medium difficulty |
| clawmacdo | Significant rewrite needed |
| Complexity | High |

---

## Option C — Detailed Cons Analysis

### Con #1: Docker Socket = Root Access to Host 🚨

**Severity: CRITICAL**

Mounting `/var/run/docker.sock` into the gateway container gives it root-equivalent access to the entire host. This is the fundamental paradox of Option C: you containerize the gateway for security, but the Docker socket mount gives back all the access you tried to restrict.

An attacker or rogue AI prompt could:
- Create a privileged container that mounts `/` (host root filesystem)
- Read `/etc/shadow`, SSH keys, all API keys on the host
- Spawn containers for cryptomining or data exfiltration
- Delete all containers, volumes, and images on the host
- Read-only socket mount does NOT help — the Docker API is still fully accessible

**Mitigations (partial):**
- **Docker socket proxy** (e.g. `docker-socket-proxy`): Restricts which Docker API endpoints are accessible. Adds complexity and another container to manage.
- **Sysbox runtime**: Allows nested containers without socket mount or privileged mode. Requires host-level installation. Not widely supported.
- **Podman**: Daemonless, rootless container engine. OpenClaw sandbox currently assumes Docker.

### Con #2: WhatsApp Pairing Fragility 📱

**Severity: HIGH**

OpenClaw uses Baileys (WhatsApp Web protocol) for WhatsApp connectivity. Inside Docker:

- Initial QR code scan requires either:
  - Manual pairing before containerizing (save session to volume)
  - noVNC setup inside container for remote QR display
  - Phone number + OTP pairing (new in 2026, but reliability unclear)
- Session data must be volume-mounted; if volume mapping breaks, re-pairing is needed
- Docker's NAT layer adds latency to WebSocket connections
- The frequent gateway disconnects (499/428/503) observed on the current host-based setup would likely **worsen** inside Docker due to additional networking layers
- Container restart = potential WhatsApp session loss if not properly persisted

### Con #3: Networking Complexity 🌐

**Severity: MEDIUM-HIGH**

- Gateway needs outbound access to: WhatsApp servers, Telegram API, Anthropic/OpenAI/Google APIs, npm registry
- Gateway health check on port 18789 (loopback) needs proper port mapping
- Sandbox containers need inter-container communication with gateway
- DNS resolution inside Docker can be unreliable
- Tailscale VPN requires `--cap-add NET_ADMIN` and `/dev/net/tun` device access
- Docker bridge networking adds 1-2ms latency per connection

### Con #4: Persistent State Management 💾

**Severity: MEDIUM**

Required volume mounts:

```yaml
volumes:
  - ./openclaw-config:/home/openclaw/.openclaw
  - ./workspace:/home/openclaw/.openclaw/workspace
  - ./media:/home/openclaw/.openclaw/media
  - /var/run/docker.sock:/var/run/docker.sock
```

Risks:
- UID/GID mismatches between host and container user
- Volume corruption if container crashes during write operations
- Backup/restore complexity increases (Docker volumes vs simple tar)
- File ownership conflicts when accessing workspace from both host and container
- Media files (voice messages, images) need proper permission handling

### Con #5: Debugging Difficulty 🐛

**Severity: MEDIUM**

| Task | Host (current) | Docker |
|------|---------------|--------|
| View logs | `journalctl --user -u openclaw-gateway` | `docker logs openclaw-gateway` |
| Restart | `systemctl --user restart openclaw-gateway` | `docker restart openclaw-gateway` |
| Shell access | Direct SSH | `docker exec -it openclaw-gateway bash` |
| Edit config | `vim ~/.openclaw/openclaw.json` | Edit volume-mounted file or exec into container |
| Install plugin | `openclaw plugins install ...` | Exec into container + restart |
| Check process | `ps aux \| grep openclaw` | `docker top openclaw-gateway` |

- Log rotation differs (Docker log driver vs journalctl)
- Container crash may lose buffered logs
- systemd integration lost (no watchdog, no automatic restart policies — must use Docker restart policies instead)

### Con #6: Performance Overhead ⚡

**Severity: LOW-MEDIUM**

- Container startup: ~1-3s overhead
- Network NAT layer: ~1-2ms per request
- Memory overhead: ~50-100MB for container runtime
- Disk I/O through overlay2 filesystem: 5-15% slower than native for write-heavy operations
- TTS audio processing slightly slower
- Playwright/Chromium screenshots slightly slower
- WebSocket keepalive timing may be affected by Docker networking

### Con #7: clawmacdo Rewrite Required 🔧

**Severity: HIGH (effort)**

Current clawmacdo performs 16 SSH-based provisioning steps. Moving to Docker would require:

- Replace Steps 9-15 with `docker compose up` or equivalent
- Build and publish a gateway Docker image (DockerHub or GHCR)
- Cloud-init simplified to: install Docker + pull image + compose up
- Web UI SSE progress would need Docker log tailing instead of SSH command output
- Backup/restore changes: Docker volumes instead of filesystem tar
- Tencent Cloud integration just completed — would need rework
- Estimated effort: 2-3 weeks to rewrite provisioning pipeline

### Con #8: Sandbox-in-Sandbox (Docker-in-Docker) 🪆

**Severity: HIGH**

OpenClaw's sandbox mode spawns Docker containers for tool isolation. If the gateway is also in Docker:

- **Option 1: Docker-in-Docker (DinD)** — Run Docker daemon inside the gateway container
  - Requires `--privileged` flag → defeats all security benefits
  - Storage driver conflicts (overlay2 inside overlay2)
  - Known stability issues
  
- **Option 2: Sibling containers** — Mount Docker socket, spawn containers alongside gateway
  - Returns to Con #1 (root access via socket)
  - Volume paths must be translated between container and host perspectives
  - Container networking between gateway and sandbox containers is complex

- **Option 3: Sysbox runtime** — True nested containers without privilege escalation
  - Requires Sysbox installation on host
  - Limited platform support
  - Not widely tested with OpenClaw

---

## Security Analysis: The Docker Socket Paradox

```
Goal:     Isolate gateway from host for security
Requires: Docker socket to spawn sandbox containers
Result:   Docker socket gives root access to host
Net:      Security improvement ≈ 0 (or negative due to added complexity)
```

This is the fundamental problem with Option C. The security benefit of containerizing the gateway is almost entirely negated by the requirement to mount the Docker socket for sandbox functionality.

A Docker socket proxy can limit API access but:
- Adds another service to manage and secure
- Must be carefully configured (allowlisting specific endpoints)
- Still allows container creation (which is the attack vector)
- Increases operational complexity

---

## Recommendation

### Short-term (now): Stay on Host ✅

The current host-based architecture is the right choice because:
1. clawmacdo is built for it and works well
2. WhatsApp pairing is seamless
3. Debugging is straightforward
4. The Docker socket paradox means containerization doesn't meaningfully improve security
5. Security hardening (allowlists, workspaceOnly, groupPolicy) provides better ROI

**Already implemented hardening:**
- Telegram dmPolicy: allowlist (only Kenneth's user ID)
- WhatsApp groupPolicy: allowlist
- workspaceOnly: true (restricts file access)
- Healthcheck script with auto-restart

### Medium-term: Monitor ecosystem

Watch for:
- OpenClaw native Podman support (rootless sandboxing)
- Sysbox maturity and OpenClaw compatibility
- WhatsApp Business API adoption (eliminates pairing issues)
- gVisor or Kata Containers integration

### Long-term: Revisit when ecosystem catches up

The right time to containerize is when:
1. OpenClaw supports rootless container runtimes (Podman/Sysbox)
2. WhatsApp Business API replaces Baileys-based pairing
3. Docker socket proxy is natively integrated into OpenClaw

---

## References

- [OpenClaw Sandboxing Docs](https://docs.openclaw.ai/gateway/sandboxing)
- [Docker Socket Security Risks](https://grokipedia.com/page/Docker_socket_security)
- [DinD Alternatives](https://devopscube.com/run-docker-in-docker/)
- [Sysbox Runtime](https://github.com/nestybox/sysbox)
- [Docker Socket Proxy](https://github.com/Tecnativa/docker-socket-proxy)
- [WhatsApp Business API](https://business.whatsapp.com/products/business-platform)
