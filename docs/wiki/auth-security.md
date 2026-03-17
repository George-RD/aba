# Loom Auth, Security & Configuration — Wiki Reference

**Source:** `docs/research/loom-auth-security-config.md` (research session, March 2026)
**Origin:** Geoffrey Huntley's [ghuntley/loom](https://github.com/ghuntley/loom) spec files

---

## Auth Providers

| Provider | Flow Type | Key Details |
|---|---|---|
| **OAuth 2.0** | Authorization Code | GitHub, Google, Okta. Account auto-links by email across providers. |
| **Magic Links** | Token in email URL | 10-minute expiry, single-use. `https://loom.dev/auth/magic-link?token=<token>` |
| **Device Code** | Poll-based headless | CLI/SSH: request code → browser confirms → poll `/auth/device-token`. 5s interval, no localhost required. |
| **Session Cookies** | Stateful web | Sliding 60-day expiry, `Secure + HttpOnly + SameSite=Strict`. Stores GeoIP for anomaly detection. |
| **API Keys** | Bearer token | Format `lt_{env}_{40+chars}`. Argon2-hashed on receipt; plaintext never stored. Scope-based. |

### Token Formats

| Type | Prefix | Validated By |
|---|---|---|
| Access token | `lt_` | Argon2 hash comparison |
| Magic link | (none) | Time window + hash |
| Session cookie | (none) | Server-side session store |
| Share link | (none) | Hash-based |

### Development Mode

`LOOM_SERVER_AUTH_DEV_MODE=1` disables all auth checks. Never in production.

---

## ABAC Authorization Model

Loom uses Attribute-Based Access Control evaluated at two layers:

1. **Route middleware** — coarse authentication gate
2. **Handler macros** — fine-grained subject/resource/action evaluation

### Subjects (who)

- User ID
- Organization memberships
- Team memberships
- Global role: `super_admin | org_admin | member | viewer`

### Resources (what)

- Owner user ID
- Associated org and team
- Visibility: `private | team | organization | public`
- Support-access flags

### Actions (how)

`Read` · `Write` · `Delete` · `Share` · `UseTool` · `UseLlm` · `ManageOrg`

### Thread Visibility Hierarchy

```
Private → Team → Organization → Public
```

Each level grants access to all subjects within that scope and all wider scopes.

### SSRF Protection

Webhooks and repository mirrors block: `127.0.0.1`, `::1`, `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `169.254.0.0/16` (including cloud metadata endpoints).

**Test coverage:** 193 passing authorization tests across org isolation, thread ownership, API key scoping, weaver access, and session revocation.

---

## `Secret<T>` Type-Safe Wrapper

Prevents plaintext secret exposure at **compile time** via Rust's type system.

```rust
pub struct Secret<T> { inner: T }

impl<T> Debug   for Secret<T> { fn fmt(..) { write!(f, "[REDACTED]") } }
impl<T> Display for Secret<T> { fn fmt(..) { write!(f, "[REDACTED]") } }
impl<T: Serialize> Serialize for Secret<T> {
    fn serialize(..) { "[REDACTED]".serialize(serializer) }
}

impl<T> Secret<T> {
    pub fn expose(&self) -> &T { &self.inner }   // explicit, grep-visible
}

impl<T: Zeroize> Drop for Secret<T> {
    fn drop(&mut self) { self.inner.zeroize(); } // zeroed on deallocation
}
```

### What it protects

- All logging (`tracing::info!("{}", api_key)` → `[REDACTED]`)
- All serde serialization (JSON responses, config snapshots)
- All debug output
- Memory after drop (via `zeroize`)

### Redaction pattern

Developers must call `.expose()` to access the value — this call is visible in code review and easy to audit with grep.

### Crate layout

| Crate | Role |
|---|---|
| `loom-secret` | Core `Secret<T>` type, no business logic |
| `loom-common-config` | `load_secret_env()`, `{VAR}_FILE` pattern |
| Domain crates (e.g. `loom-server`) | Use `Secret<String>` for API keys |

### File-based injection (`{VAR}_FILE`)

```bash
LOOM_SERVER_ANTHROPIC_API_KEY_FILE=/run/secrets/anthropic_key
# Takes precedence over LOOM_SERVER_ANTHROPIC_API_KEY
```

---

## Configuration Layering

Six tiers, lowest to highest precedence:

| Tier | Source | Precedence |
|---|---|---|
| 1 | Built-in defaults | 10 |
| 2 | System config `/etc/loom/config.toml` | 20 |
| 3 | User config `~/.config/loom/config.toml` (XDG) | 30 |
| 4 | Workspace config `.loom/config.toml` | 40 |
| 5 | Environment variables `LOOM_*` | 50 |
| 6 | CLI arguments | 60 (highest) |

### XDG paths

| Purpose | Default |
|---|---|
| Config | `~/.config/loom/config.toml` |
| Data | `~/.local/share/loom/` |
| State | `~/.local/state/loom/` |
| Cache | `~/.cache/loom/` |

Each path overridable via `LOOM_CONFIG_HOME`, `LOOM_DATA_HOME`, etc.

### Env var naming convention

`{SECTION}_{KEY}` under prefix `LOOM_`. Examples:

| Config path | Env var |
|---|---|
| `server.url` | `LOOM_SERVER_URL` |
| `providers.anthropic.api_key` | `LOOM_SERVER_ANTHROPIC_API_KEY` |
| `auth.token` | `LOOM_AUTH_TOKEN` |
| `weaver.ttl_seconds` | `LOOM_WEAVER_TTL_SECONDS` |

### Merge semantics

- **Tables:** deep merge (higher wins per key)
- **Scalars:** full replacement
- **Arrays:** full replacement (no element-wise merge)

### Auto-configuration

First launch with no config generates a commented-out template at the XDG config path.

---

## Audit System

Non-blocking fan-out architecture. Audit events never block request handling.

### Event flow

```
Request handler
    │
    ▼
Bounded MPSC channel (10,000 capacity)
    │
    ▼
Async enrichment (GeoIP, org context, session metadata)
    │
    ▼
Global filter (min severity, excluded event types)
    │
    ├─▶ SQLite (primary, 90-day retention)
    ├─▶ Syslog RFC 5424
    ├─▶ HTTP webhooks (batched)
    ├─▶ JSON Lines file stream
    ├─▶ OpenTelemetry spans
    ├─▶ CEF → Splunk HEC
    ├─▶ Datadog agent
    └─▶ Elasticsearch
```

### `AuditLogEntry` fields

`actor_id` · `organization_id` · `session_id` · `action` · `resource_type` · `resource_id` · `timestamp` · `ip_address` · `user_agent` · `geo_location` · `trace_id` · `span_id` · `request_id` · `status` (Success/Failure/Denied) · `result_message`

### Severity levels (RFC 5424)

| Level | Examples |
|---|---|
| Debug (7) | Config changes, internal ops |
| Info (6) | Successful API calls, new resources |
| Notice (5) | auth.login, policy changes |
| Warning (4) | Rate limiting, unusual activity |
| Error (3) | API errors, permission denied |
| Critical (2) | token_revoked, unauthorized_access |
| Alert (1) | Compromised credentials, incident response |

### Queue overflow policy

Default: drop oldest, increment Prometheus counter. Configurable to drop newest or apply backpressure.

### Graceful degradation

- **Transient errors** (5xx, 429, timeout): exponential backoff, 3 retries, then drop + log
- **Permanent errors** (401, 403): disable sink, re-enable on next health check
- **Slow sinks:** non-blocking, dropped events increment metric, alerts on drop rate

### SIEM integration

| Platform | Sink type |
|---|---|
| Splunk | `splunk_hec` (HTTP Event Collector) |
| Datadog | `datadog` (DD API key) |
| Elastic/ELK | `elasticsearch` (direct or Logstash) |
| Generic | `http` webhook, `json_stream` (JSONL file), `syslog` (CEF) |

---

## Feature Flags & Kill Switches

### Two-tier architecture

| Tier | Managed by | Scope | Evaluated |
|---|---|---|---|
| Platform flags | Super admins | All orgs | 3rd in precedence |
| Org flags | Org admins | Single org | 4th in precedence |

Kill switches exist at both tiers and are evaluated **first** (highest precedence), before any rollout logic.

### Evaluation order (per request)

1. Platform-level kill switches (emergency stop)
2. Org-level kill switches
3. Platform-level flags
4. Org-level flags
5. Local SDK defaults

### Targeting rules

`Attribute` · `Percentage` (murmur3 hash for consistency across requests) · `Geographic` (GeoIP) · `Environment` · `TimeWindow`

### A/B variants

Each flag has named variants (e.g. `control` / `treatment_a` / `treatment_b`) with percentage split. `rollout_percentage` gates total exposure.

### Real-time updates

Flag changes push immediately via Server-Sent Events (SSE) on `/api/flags/stream`. Clients update their local evaluation cache without polling.

### Kill switch behaviour

- Zero latency (evaluated before other flags)
- No caching, always fresh
- Every activation logged at `severity: critical`
- Supports cascading prerequisites

---

## Redaction Patterns

Beyond `Secret<T>`, Loom applies redaction at multiple layers:

| Layer | Mechanism |
|---|---|
| Rust logging | `Debug`/`Display` impls print `[REDACTED]` |
| JSON serialization | `Serialize` impl emits `"[REDACTED]"` |
| Config file snapshot | Secrets serialized via `Secret<T>` |
| Weaver secrets API | Returns `Secret<String>`, never env vars |
| Database storage | Argon2 hash only, no plaintext |
| File permissions | Config files at `0600` |
| Kubernetes secrets | etcd encryption at rest + file-based `_FILE` injection |

**NEW INSIGHT:** Secrets inside weavers are fetched via an authenticated API call (not environment variables) so `env | grep PASSWORD` cannot leak them, and access is individually audited per trace ID.

---

## Key Insights for ABA

**1. `Secret<T>` is the highest-value pattern to adopt immediately.**
ABA already handles API keys in `config.rs`. Wrapping them in a `Secret<T>` type (even a minimal one) prevents accidental logging during the Ralph loop. The `.expose()` call convention makes secret access grep-auditable.

**2. The 6-tier config precedence matches ABA's current approach.**
ABA uses `~/.config/ABA/config.toml` via the `directories` crate, which is tier 3 (XDG user config). Adding `LOOM_`-style env var overrides at tier 5 would be a clean, additive improvement for CI/deployment.

**3. Non-blocking audit is the right pattern for agent loops.**
An agent loop running `cargo test` dozens of times benefits from audit events that don't slow iteration. MPSC + background task is directly applicable in `agent.rs`.

**4. Kill switches map naturally to the Ralph loop.**
A kill switch that aborts `loop.sh` mid-run (e.g. on quota exhaustion or test failures exceeding a threshold) would be a practical safety mechanism. Currently `loop.sh` has only a max-iterations guard.

**5. OAuth pool failover is directly reusable in ABA.**
ABA's `OpenAiOAuthClient` could implement the same three-state machine (`Available` / `CoolingDown` / `Disabled`) with murmur3-based round-robin selection across pooled credentials.

**6. ABAC subject/resource/action is overkill for ABA today, but the vocabulary is useful.**
When ABA gains multi-user or multi-project capabilities, adopting the same `(subject, resource, action)` evaluation model avoids re-inventing authorization semantics.

**NEW INSIGHT — Device Code Flow is already solved for CLI.**
Loom's device code flow (`/auth/device-code` → poll `/auth/device-token`) is identical to what ABA partially implements for OpenAI OAuth. Loom's approach (no localhost binding, human-readable code, 5s poll) is cleaner than ABA's current implementation and worth adopting verbatim.

**NEW INSIGHT — Envelope encryption (KEK/DEK) is the right path for stored secrets.**
ABA currently stores tokens in plaintext in `~/.config/ABA/config.toml`. Loom's three-layer scheme (KEK in OS keychain or K8s secret → per-secret DEK → AES-256-GCM ciphertext) allows key rotation without re-prompting the user.

---

## Sources

- [Auth & ABAC System Spec](https://github.com/ghuntley/loom/blob/trunk/specs/auth-abac-system.md)
- [Audit System Spec](https://github.com/ghuntley/loom/blob/trunk/specs/audit-system.md)
- [Configuration System Spec](https://github.com/ghuntley/loom/blob/trunk/specs/configuration-system.md)
- [Feature Flags System Spec](https://github.com/ghuntley/loom/blob/trunk/specs/feature-flags-system.md)
- [Secret System Spec](https://github.com/ghuntley/loom/blob/trunk/specs/secret-system.md)
- [Weaver Secrets System Spec](https://github.com/ghuntley/loom/blob/trunk/specs/weaver-secrets-system.md)
- [Weaver eBPF Audit Spec](https://github.com/ghuntley/loom/blob/trunk/specs/weaver-ebpf-audit.md)
- [Anthropic OAuth Pool Spec](https://github.com/ghuntley/loom/blob/trunk/specs/anthropic-oauth-pool.md)
- [SCIM System Spec](https://github.com/ghuntley/loom/blob/trunk/specs/scim-system.md)
- [Verification Report](https://github.com/ghuntley/loom/blob/trunk/verification.md)
