# Loom Observability & Analytics — Wiki Reference

*Compressed from exhaustive research into ghuntley/loom's observability stack.*
*Date: 2026-03-17 | Source: `/docs/research/loom-analytics-observability.md` + `/LOOM_OBSERVABILITY_ARCHITECTURE.md`*

---

## Overview

Loom embeds a self-hosted observability platform spanning analytics, crash tracking, session health, cron monitoring, structured logging, and cost accounting — all implemented as a set of **30+ specialized Rust crates**. The guiding constraint: observability must never cascade-fail into the agent loop. If any subsystem fails, the agent logs and continues.

---

## Analytics System (PostHog-Style, Self-Hosted)

Loom does **not** use PostHog; it implements the same design pattern internally.

### Core Crates

| Crate | Role |
|-------|------|
| `loom-analytics` | Main SDK and event pipeline |
| `loom-analytics-core` | Core types and event structures |
| `loom-server` | HTTP API with analytics endpoints |
| `loom-api` | REST surface exposing analytics |

### Event API

```
capture(event_name, properties)     → Single event
batch_capture(events[])             → Bulk ingestion
identify(distinct_id, properties)   → Associate user identity
alias(previous_id, new_id)          → Link anonymous → identified
set_properties(user_id, properties) → Retroactive property update
```

### Capture Modes

1. **Direct** — immediate HTTP POST for critical events
2. **Batch Queue** — accumulate and flush on size or interval
3. **Offline Queue** — localStorage/IndexedDB fallback when network is unavailable
4. **Circuit Breaker** — stop sending if endpoint is failing; never cascade

### Standard Properties (auto-populated)

`$timestamp`, `$session_id`, `$distinct_id`, `$sdk_version`, `$os`, `$agent_version`

Custom properties: up to 1000 per event, nested objects (flattened for query), PII-filtered on capture.

### API Keys

| Prefix | Scope |
|--------|-------|
| `pk_live_*` | capture, identify, alias, set_properties (write-only) |
| `pk_crash_*` | crash reporting only |
| `sk_live_*` | full admin (never shared with SDKs) |

---

## Identity Resolution

### Anonymous → Identified Flow

```
anonymous_id generated on first event
  → user logs in
  → identify(email, {name, org, ...})
  → alias(anonymous_id, user_id)       ← links event histories
  → person merge: de-duplicate properties, retroactive backfill
```

### API

```
POST /api/analytics/identify
{ "distinct_id": "user@example.com", "properties": { ... } }

POST /api/analytics/alias
{ "distinct_id": "user@example.com", "alias": "anon-abc123" }
```

---

## Crash Tracking

### Core Crates

`loom-crashes-core`, `loom-crashes` (SDK), `loom-crashes-api`

### Crash Lifecycle

```
panic / unhandled exception
  → collect: stack trace (symbolicated), breadcrumbs (last 50 log lines),
             environment context, last 5 LLM messages
  → deduplicate: hash stack trace → fingerprint
  → POST /api/crashes/report
  → create new issue or append to existing fingerprint group
  → alert if new fingerprint or high crash velocity
```

### Crash-Free Rate

```
crash_free_percent = (sessions_without_crash / total_sessions) × 100
```

Dimensions: by version, environment (staging/prod), agent type, rolling window (24h, 7d, 30d).

### Stack Trace Handling

- **Symbolication**: binary offset → `source_file:line:column`
- **Deduplication**: normalized stack hash for grouping
- **Breadcrumb trail**: 50 structured log entries preceding the crash
- **Source map**: TypeScript original locations
- **Reproduction context**: recent agent messages and tool calls

### Crash API

```
POST /api/crashes/report      → submit crash
GET  /api/crashes             → list (filter: version, status, assignee)
GET  /api/crashes/{id}        → full details + related events
PATCH /api/crashes/{id}       → resolve, assign
```

---

## Session Health

### Session Lifecycle

```
agent_started → generate session_id (UUID v4) → [active]
  → events logged, health monitored per-turn
  → end triggers: goal complete | user quit | timeout (>12h) | crash
  → compute metrics: duration, event count, crashes, tool calls, cost
  → archive session
```

### Health Grades

| Grade | Threshold | State |
|-------|-----------|-------|
| A | 90–100% | Normal operation |
| B | 70–89% | Warnings present |
| C | 50–69% | Degraded |
| D | <50% | Critical |

### Health Indicators

- **Status**: `running`, `completed`, `failed`, `timeout`, `interrupted`
- **Event rate**: events/minute — stall if < 0.1 eps
- **Tool success rate**: successful calls / total calls
- **Cost efficiency**: tokens per task completion
- **Error rate**: errors per 1000 events

### Session API

```
GET /api/sessions                         → list (filter: status, user_id, date)
GET /api/sessions/{id}                    → full detail: timeline, tools, crashes, cost
GET /api/sessions/{id}/timeline           → ordered event stream
```

---

## Cron Monitoring

### Core Crates

`loom-crons-core`, `loom-server-crons`, `loom-crons-api`

### Failure Types Detected

1. **Missed run** — scheduled time passed, no execution
2. **Timeout** — exceeded `max_duration`
3. **Non-zero exit** — process returned error code
4. **Silent failure** — exited but logs missing or corrupt
5. **Performance degradation** — execution time > 2σ above historical mean

### Alerting Escalation

```
1st miss  → log warning, mark "missed_once"
2nd miss (24h) → alert (email/Slack), mark "monitoring_required"
3rd miss (24h) → escalate, trigger fallback handler
```

### Cron API

```
GET /api/crons                            → list with last_run, next_run, status
GET /api/crons/{id}/executions            → execution history (filter: date, status)
GET /api/crons/{id}/health                → success rate, avg/P95 duration, missed count
```

---

## Health Endpoint (`/health`)

### Specification

- **Method**: `GET /health`
- **Auth**: None (public endpoint)
- **Status codes**: `200 OK` (healthy or degraded), `503` (unhealthy)

### Response Format

```json
{
  "status": "healthy",
  "timestamp": "2026-03-17T14:30:45Z",
  "version": "1.2.0",
  "components": {
    "api_server":   { "status": "healthy",  "latency_ms": 2    },
    "database":     { "status": "healthy",  "latency_ms": 8,   "connections": 42 },
    "llm_provider": { "status": "degraded", "latency_ms": 1250, "error_rate_percent": 2.3 },
    "analytics":    { "status": "healthy",  "latency_ms": 5,   "queue_size": 1234 },
    "cache":        { "status": "healthy",  "latency_ms": 1,   "hit_rate_percent": 87.3 }
  },
  "metrics": {
    "uptime_seconds": 86400,
    "memory_mb": 512,
    "memory_limit_mb": 2048,
    "disk_free_gb": 42
  }
}
```

### Component Health Trait

```rust
pub trait HealthCheck {
    async fn health(&self) -> ComponentHealth;
}
pub struct ComponentHealth {
    status: Status,   // Healthy | Degraded { reason } | Unhealthy { reason }
    latency_ms: u32,
    last_check: DateTime<Utc>,
    details: serde_json::Value,
}
```

### Check Intervals

| Interval | Components |
|----------|-----------|
| Fast (10s) | API server, cache, queue |
| Medium (30s) | Database, analytics |
| Slow (5m) | LLM provider, external APIs |

### Cascading Failure Behaviour

- **LLM unhealthy** → mark agent "degraded", queue restart, alert ops
- **Database unhealthy** → mark system "unhealthy", halt new sessions, trigger failover
- **Analytics unhealthy** → agent continues; retry with exponential backoff (graceful degradation)

---

## Cost Tracking

### `Usage` Struct

```rust
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    pub cost_usd: f64,
}
```

### `CostTracker`

Holds a `PricingTable` keyed by `(LlmProvider, model)` with rates per 1M tokens.

```rust
// Pricing (per 1M tokens: input / output)
anthropic: { "claude-opus-4" => (3.00, 15.00), "claude-haiku-4" => (0.80, 4.00) }
openai:    { "gpt-4"         => (3.00,  6.00), "gpt-4-turbo"    => (1.00, 3.00) }

cost = (input_tokens / 1_000_000) × input_rate
     + (output_tokens / 1_000_000) × output_rate
```

Usage is logged via `tracing::info!` (provider, model, input_tokens, output_tokens, cost_usd) on every LLM call and aggregated by agent and session.

---

## Structured Logging (`tracing`)

### Setup

```rust
tracing_subscriber::fmt()
    .with_max_level(Level::INFO)
    .with_target(true)
    .with_thread_ids(true)
    .json()
    .init();
```

### Instrumentation Pattern

```rust
#[instrument(skip(self, secrets, large_args), fields(id = %id))]
pub async fn execute_tool(&self, id: &str, tool_name: &str, args: &Value, secrets: &SecretMap)
```

Key rule: **always `skip` secrets and large buffers** — they must never appear in log output.

### Log Levels

| Level | Usage |
|-------|-------|
| TRACE | Detailed control flow (disabled in production) |
| DEBUG | Function entry/exit, variable values (dev only) |
| INFO | User actions, agent progress, milestones |
| WARN | Degraded operation, retries, timeouts |
| ERROR | Failures, exceptions, unrecoverable states |

### Contextual Spans

```rust
let span = span!(Level::INFO, "agent_execution",
    session_id = %session.id,
    goal = %goal,
    turn = Empty,
);
// All logs inside span carry session_id, goal, turn automatically
span.record("turn", turn);
```

### JSON Log Shape

```json
{
  "timestamp": "...", "level": "INFO", "target": "loom_agent",
  "fields": { "message": "tool completed", "tool": "bash",
               "session_id": "sess_123", "turn": 5,
               "success": true, "duration_ms": 1250 },
  "spans": [{ "name": "agent_execution", "session_id": "sess_123", "goal": "..." }]
}
```

---

## Self-Healing Ralph Loop

### Pattern

```
[1] Monitor /health + metrics
[2] Detect: component unhealthy? error rate spike? perf degradation?
[3] Analyze root cause via structured logs + crash breadcrumbs
[4] Generate patch with LLM, test in staging
[5] Deploy: auto-commit + push → CI/CD pipeline
[6] Verify: confirm metrics recovered
[7] On failure: retry once, then escalate + circuit-break
```

Health loop polls every 30 seconds. Implementation in `system_health_loop()`.

### January 2026 Achievement

The first autonomous end-to-end auto-heal of a production bug — **zero human intervention**:

| Time | Event |
|------|-------|
| 14:30 | Health loop detected crash-free rate drop: 98.5% → 92.3%; fingerprint "session timeout panic" |
| 14:32 | Agent traced root cause to `sessions/core.rs:187` — timeout integer overflow |
| 14:35 | Agent generated fix: bounded timeout calculation + regression test |
| 14:38 | Full test suite passed (427 tests); deployed to staging; crash rate → 0% |
| 14:41 | Auto-committed with extended metadata, pushed to main, triggered CI/CD |
| 14:50 | Post-deploy: crash-free 98.7%, session health 99.2%, issue auto-closed |

**Total time: 20 minutes. Human involvement: none.**

### Auto-Commit Metadata

Every auto-commit carries structured metadata in the commit message body:

```
Agent: build-v2  |  Session: sess_456def  |  Turn: 7  |  Cost: $0.23
Tests: 427 passed, 0 failed  |  Crash-Free: verified
```

The `AutoCommitResult` struct captures: `commit_id`, `files_changed`, `insertions`, `deletions`, `tests_passed`, `agent_id`, `session_id`.

---

## Observability UI (Svelte 5 Dashboard)

Routes: `/health`, `/analytics`, `/crashes`, `/sessions`, `/crons`

Real-time event streaming via **SSE** (`/api/sessions/{id}/events/stream`). Svelte 5 `$state` + `$effect.pre` for reactive metric updates.

Key views:
- **Health dashboard** — component-level uptime, live event feed
- **Crash explorer** — grouped by fingerprint, breadcrumb playback, stack trace browser
- **Session timeline** — chronological tool log, cost breakdown, error highlighting
- **Cron monitor** — next/last run, success rate, P95 duration, missed-run history

---

## Key Insights for ABA

**NEW — not previously documented in ABA's codebase:**

1. **Graceful degradation is non-negotiable.** Analytics failures must never propagate to the agent loop. The pattern is `if let Err(e) = analytics.capture(...) { warn!(...); }` — log and continue, always.

2. **Session health grading (A–D) is a forcing function.** Grade D (<50%) should trigger automatic loop pause and alert rather than silent degradation. ABA's loop has no equivalent yet.

3. **The `skip(secrets, large_args)` tracing pattern.** ABA's `#[instrument]` annotations should explicitly name what to skip — the current codebase may be logging oversized tool outputs into spans.

4. **Cost tracking needs a per-session `Usage` accumulator.** ABA currently logs token counts but does not aggregate cost_usd or expose it in commit metadata. The Jan 2026 auto-heal commit demonstrates cost as a first-class field in git history.

5. **The self-healing loop (Jan 2026)** proves the architecture works at production scale. The critical enabler was the combination of: breadcrumbs (50 log lines before crash) + structured spans (session_id/turn in every log line) + crash fingerprinting. Without all three, the agent cannot diagnose root cause autonomously.

6. **`HealthCheck` as a trait, not a function.** Each ABA subsystem (LLM client, VCS, config) should implement a common `HealthCheck` trait so a future `/health` endpoint can aggregate them uniformly.

7. **Cron monitoring closes the loop.** ABA's `loop.sh` runs on a schedule but has no missed-run detection. A lightweight cron monitor would catch silent failures in CI/CD and the outer Ralph loop itself.

---

## References

- [ghuntley/loom — GitHub](https://github.com/ghuntley/loom)
- [Everything is a Ralph Loop](https://ghuntley.com/loop/)
- [Inventing the Ralph Wiggum Loop (Podcast)](https://linearb.io/dev-interrupted/podcast/inventing-the-ralph-wiggum-loop)
- [LinearB: Mastering Ralph Loops](https://linearb.io/blog/ralph-loop-agentic-engineering-geoffrey-huntley)
- Full architecture: `/LOOM_OBSERVABILITY_ARCHITECTURE.md` (~1640 lines)
