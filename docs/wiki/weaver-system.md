# Weaver System

*Wiki reference compressed from research into ghuntley/loom. Date: 2026-03-17.*

---

## What Is a Weaver?

A **Weaver** is an ephemeral Kubernetes pod running a Loom REPL for remote code execution and interactive agent sessions. Weavers are the distributed execution environment within Loom: agents spin up, perform work via tool calls, and tear down. Kubernetes is the sole source of truth — no separate database tracks weaver state.

**Max concurrent weavers**: 64 (enforced via ABAC quota policy or K8s ResourceQuota in the `loom-weavers` namespace).

---

## Creation Flow

```
loom new --image <img> [--ttl <hours>] [--repo <git-url>] [-e VAR=val]
  |
  | POST /api/weaver  (SPIFFE bearer token)
  v
loom-server  →  loom-server-k8s crate
  |
  | validate image, resource constraints
  | generate pod name: weaver-{uuid7}
  v
Kubernetes pod manifest created (security hardening + TTL annotation)
  |
  v
Pod scheduled → container starts → Loom REPL entrypoint
  |
  v
loom-cli attaches via WebSocket  (kubectl-exec equivalent)
  |
  | User detaches: Ctrl+P, Ctrl+Q  (pod keeps running)
  v
TTL expires or `loom weaver delete <id>`
  |
  v
Pod enters Terminating → SIGTERM → 30s grace → SIGKILL → removed from etcd
  |
  v
Webhook: weaver.deleted fired
```

**Step-by-step**:

1. CLI sends `POST /api/weaver` with image, TTL, repo URL, env vars, and SPIFFE identity token.
2. `loom-server-k8s` validates the request, generates `weaver-{uuid7}` pod name.
3. Pod manifest is built with all security hardening applied and `loom.dev/ttl` annotation set.
4. Pod is scheduled; container starts with the Loom REPL as entrypoint.
5. CLI attaches via WebSocket — interactive REPL is live.
6. User detaches with `Ctrl+P, Ctrl+Q`; pod survives detach.
7. Background TTL controller deletes pod when `creation_time + ttl < now()`.

---

## Pod Specification

### Security Context

```yaml
securityContext:
  runAsNonRoot: true
  readOnlyRootFilesystem: true
  allowPrivilegeEscalation: false
  capabilities:
    drop:
      - ALL
    add: []
```

Conforms to Kubernetes `restricted` Pod Security Standard (strictest profile).

### Resource Limits

| Resource | Default |
|----------|---------|
| Memory   | 16 GB   |
| CPU      | No limit (unrestricted) |
| Storage  | Ephemeral only (no PVCs) |

### Volumes

| Mount      | Type      | Purpose                        |
|------------|-----------|--------------------------------|
| `/workspace` | emptyDir | Git clone target             |
| `/tmp`       | emptyDir | Audit buffer + scratch       |
| `/`          | read-only | Immutable root filesystem    |

### Pod Metadata

```
Name:       weaver-{uuid7}
Namespace:  loom-weavers
Labels:
  app: loom-weaver
  weaver-id: {uuid7}
  owner: {user-id}
  created-at: {timestamp}
Annotations:
  loom.dev/ttl: <seconds>
```

### TTL Cleanup

- **Default TTL**: 4 hours (14 400 s)
- **Maximum**: 48 hours (172 800 s)
- **Mechanism**: Background controller reads `loom.dev/ttl` annotation; deletes pod when expired.
- **Grace period**: 30 s (standard Kubernetes termination grace).

---

## CLI Commands

```bash
# Create
loom new --image <image>
loom new --image <image> --ttl 24
loom new --repo <git-url> --branch main
loom new -e VAR=value -e VAR2=val2

# List
loom weaver ps
loom weaver ps --json

# Attach
loom attach <id>          # WebSocket attach (prefix matching supported)
loom ssh <id>             # SSH via WireGuard tunnel

# Delete
loom weaver delete <id>   # Graceful termination

# Auth / server
loom login --url https://loom.example.com
loom --server-url https://... weaver ps
```

---

## eBPF Audit Sidecar

### Architecture

- Sidecar container in the same pod, sharing process namespace.
- Requires Kubernetes 1.28+ native sidecar support.
- Sidecar must reach `Ready` before the weaver container starts.
- Output: JSONL to `POST /internal/weaver-audit/events` (authenticated via SPIFFE).
- Buffer: `/tmp/audit-buffer.jsonl`, max 256 MB; flushed in batch on reconnect.

### Sidecar Security Context

```yaml
securityContext:
  capabilities:
    add:
      - CAP_BPF
      - CAP_PERFMON
```

### Syscall Coverage

| Category             | Syscalls Monitored                                              |
|----------------------|-----------------------------------------------------------------|
| Process execution    | `execve`, `fork`, `vfork`, `exit`, `exit_group`                |
| File writes          | `write`, `pwrite`, `writev`                                     |
| File metadata        | `chmod`, `chown`, `mkdir`, `unlink`, `rename`                   |
| Network — sockets    | `socket`, `connect`, `accept`                                   |
| Network — DNS        | DNS resolution queries (full lifecycle tracking)                |
| Privilege escalation | `setuid`, `seteuid`, `setgid`, `prctl`, `capset`               |
| Memory execution     | `mmap`, `mprotect` (flagged when `PROT_EXEC` set)              |

### Performance

- Kernel-side eBPF filtering runs before userspace, reducing overhead.
- Userspace filtering excludes high-volume low-signal events (libc reads, `/proc`, `/sys`).
- High-volume operations are sampled.

### Connection State Tracking

- State machine tracks socket lifecycle: created → connected → closed.
- DNS cache enriches connection events with resolved hostnames.
- Events correlated to originating PID.

---

## WireGuard + DERP Architecture

**Principle**: "P2P when possible, relay when needed."

```
User device (loom-wgtunnel-client / Boringtun)
    |
    |-- direct WireGuard tunnel (when NAT allows)
    |-- or DERP relay (loom-wgtunnel-server) when blocked
    |
Weaver pod (unique ephemeral WireGuard keypair)
```

**Key exchange flow**:

1. `loom ssh <id>` fetches weaver pod IP from K8s API.
2. loom-cli exchanges keys with loom-server (authenticated via SPIFFE SVID).
3. WireGuard tunnel established (P2P or DERP relay fallback).
4. SSH session runs over tunnel.
5. Tunnel torn down on disconnect.

**Implementation crates**:

| Crate                     | Role                                      |
|---------------------------|-------------------------------------------|
| `loom-wgtunnel-client`    | User-side WireGuard engine (Boringtun)    |
| `loom-wgtunnel-server`    | DERP relay coordination                   |
| `loom-wgtunnel-protocol`  | Wire protocol for key exchange            |

loom-server coordinates key exchange but never sees the encrypted traffic itself.

---

## SPIFFE Identity and Secret Scoping

### Identity Format

```
spiffe://loom.dev/weaver/{weaver-id}
```

SVID (JWT): embeds weaver ID, org, repo, pod UID binding. Lifetime: **15 minutes** (auto-rotated).

### Secret Scoping Hierarchy

Precedence is most-specific first:

| Level        | Path                                | Scope                          |
|--------------|-------------------------------------|--------------------------------|
| Weaver       | `secrets.loom.dev/weaver/{weaver-id}` | Single weaver instance (wins) |
| Repository   | `secrets.loom.dev/repo/{repo-id}`     | All weavers in repo           |
| Organization | `secrets.loom.dev/org/{org-id}`       | Org-wide fallback             |

### Secret Retrieval Flow

1. REPL: `GET /api/secrets/{secret-name}` with SPIFFE SVID header.
2. loom-server validates SVID signature and claims.
3. ABAC policy check: is this weaver authorized?
4. Resolution: weaver-level → repo-level → org-level.
5. Envelope decryption via master key (HSM/KMS backend).
6. Plaintext returned; audit log entry written (value excluded).

### Encryption

- **Envelope encryption**: per-secret data keys encrypted under master key.
- **Master key**: stored in pluggable HSM/KMS backend.
- **`KeyBackend` trait backends**: in-memory (dev), local FS, AWS KMS, GCP KMS, PKCS#11.

### Threat Model

| Threat                  | Mitigation                                                             |
|-------------------------|------------------------------------------------------------------------|
| Compromised container   | Secrets scoped to weaver ID; SVID expires in 15 min                   |
| Lateral movement        | SPIFFE identity bound to pod UID; no impersonation possible           |
| Database breach         | Ciphertext useless without master key                                  |

---

## Webhook Events

| Event             | Fired When                              | Key Payload Fields                                    |
|-------------------|-----------------------------------------|-------------------------------------------------------|
| `weaver.created`  | Pod reaches `Running`                   | weaver ID, image, owner, TTL, creation timestamp      |
| `weaver.deleted`  | Pod removed from K8s                    | weaver ID, owner, reason (ttl-expired / manual / failed) |
| `weaver.failed`   | Pod enters `Failed` (crash/eviction)    | weaver ID, owner, failure reason (OOMKilled, ErrImagePull, …) |

Delivery: HMAC-SHA256 signed, exponential-backoff retry, GitHub-compatible JSON.

---

## NixOS Container Mapping (K8s Concept → NixOS Equivalent)

| K8s Concept           | NixOS Equivalent                                |
|-----------------------|-------------------------------------------------|
| `kubectl delete pod`  | `systemd-run --transient`                       |
| YAML Pod manifest     | Nix `flake.nix` + container config              |
| cgroups + namespaces  | systemd transient services + cgroups            |
| etcd source of truth  | systemd-managed lifecycle                       |
| K8s TTL controller    | systemd on-demand cleanup                       |
| K8s RBAC + env vars   | Nix secret store + systemd environment          |
| K8s audit log         | systemd journal                                 |
| Multi-node cluster    | Single machine or systemd-nspawn network        |

**NixOS advantages**: reproducible builds, lighter overhead, declarative OS config, atomic rollback, no image registry needed.

**NixOS migration blockers**: container runtime abstraction (currently K8s-specific), SPIFFE credential injection, audit backend abstraction (K8s audit log → systemd journal).

---

## LLM Call Proxy

All LLM calls from inside a weaver are proxied through loom-server. API keys never enter the weaver environment.

```
Weaver REPL
  POST /api/llm/complete  (SPIFFE SVID header)
    → loom-server validates SVID
    → ABAC policy check
    → injects stored API key
    → forwards to Anthropic / OpenAI
    → relays response back
    → audit log: {weaver_id, model, tokens, timestamp}
```

Rate limiting and provider switching happen server-side without weaver config changes.

---

## Kubernetes as Source of Truth

No separate database for weaver state. Everything lives in K8s etcd:

- Pod creation timestamp (TTL derivation)
- Owner identity (RBAC labels)
- Container status (Pending / Running / Succeeded / Failed)
- Pod IP (network routing)
- Resource usage (metrics)

Benefits: no sync issues, no extra DB infrastructure, all mutations in K8s audit log, automatic pod rescheduling on node failure.

---

## Key Repository Files

| Path                              | Content                                               |
|-----------------------------------|-------------------------------------------------------|
| `specs/weaver-provisioner.md`     | K8s pod spec, TTL, cleanup, resource limits           |
| `specs/weaver-cli.md`             | CLI command reference                                 |
| `specs/wgtunnel-system.md`        | WireGuard + DERP relay architecture                   |
| `specs/weaver-secrets-system.md`  | SPIFFE identity, envelope encryption, scoping         |
| `specs/weaver-ebpf-audit.md`      | eBPF sidecar, syscall table, buffering                |
| `crates/loom-server-k8s/`         | Kubernetes API client, pod provisioning               |
| `crates/loom-server-weaver/`      | Weaver REST API endpoints                             |
| `crates/loom-wgtunnel-*/`         | WireGuard client, server, protocol                    |
| `crates/loom-weaver-ebpf/`        | eBPF program for syscall capture                      |
| `crates/loom-weaver-audit-sidecar/` | Userspace audit event collection and relay          |

---

## New Insights (Noticed During Compression)

1. **Busybox entrypoint trap**: A container without a long-running process immediately exits with `Succeeded` status. Loom's container image requirement is non-obvious — the Loom REPL must be the entrypoint, not a one-shot command. This affects ABA if it ever spawns its own execution containers.

2. **SVID pod-UID binding**: The SPIFFE SVID is bound to the pod UID, not just the weaver ID. This means even if an attacker extracts the JWT, they cannot replay it from a different pod. This is a stronger guarantee than simple bearer tokens.

3. **Audit buffer ordering guarantee**: The 256 MB local buffer is flushed in batch *before* new events are processed on reconnect. This preserves strict causal ordering in the audit log even across disconnects — a property most audit systems sacrifice for throughput.

4. **No CPU limits by default**: Memory is capped at 16 GB but CPU is unconstrained. This is intentional for LLM inference workloads (token generation is CPU-bound and bursty). A per-user CPU quota likely lives in the ABAC layer, not the pod spec.

5. **loom-server never sees WireGuard traffic**: Key exchange is coordinated through loom-server (authenticated), but encrypted tunnel traffic flows peer-to-peer. The server is a key broker, not a VPN concentrator.

6. **K8s-as-database eliminates a whole failure mode**: The design choice to keep no separate weaver state database means there is no scenario where the DB and K8s disagree. The TTL controller reads pod annotations directly — this is elegant and worth emulating in ABA's agent loop state management.

---

## Key Insights for ABA

1. **ABA's `Vcs` trait mirrors Loom's abstraction philosophy.** Loom abstracts its container runtime behind a trait to enable K8s → NixOS substitution. ABA should apply the same pattern to its execution environment if it ever needs to run agent loops in isolated containers.

2. **The K8s-as-source-of-truth pattern maps to ABA's git-as-source-of-truth.** ABA already uses git commits as the persistent state between Ralph loop iterations. Weavers do the same with etcd. Both avoid in-memory or DB state that diverges from the canonical record.

3. **eBPF audit coverage is the security primitive that makes Weaver safe for untrusted code.** If ABA evolves to run generated code in sandboxes, syscall-level auditing (not just stdout capture) is the correct level of observation.

4. **The LLM proxy pattern solves ABA's API key problem at scale.** ABA currently reads API keys from `~/.config/ABA/config.toml`. If ABA ever becomes multi-user or cloud-hosted, routing LLM calls through a server-side proxy (injecting credentials server-side) is the right architecture — not distributing keys to each agent instance.

5. **Secret scoping hierarchy (weaver → repo → org) is directly applicable to ABA's prompt system.** ABA reads a single prompt from stdin. A scoped secrets/prompt system (task → project → global) would let engineers manage many concurrent Ralph loops without per-loop configuration.

6. **TTL-based cleanup is the right default for ephemeral agents.** ABA's `git_reset_hard()` on failure is a primitive version of this. A TTL on the agent loop itself (kill after N hours regardless of progress) would prevent runaway loops.
