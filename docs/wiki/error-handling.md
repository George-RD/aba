# Error Handling & Recovery

**Source**: Research session on `ghuntley/loom` — specs, source, Ralph loop documentation.
**Philosophy**: Prevention > Recovery. Loom avoids catching errors after the fact; it designs systems so error states don't arise.

---

## Error Classification

| HTTP Code | Classification | Action |
|-----------|---------------|--------|
| 408 | Transient | Retry with backoff |
| 429 | Transient | Retry with backoff (rate limit) |
| 500, 502, 503, 504 | Transient | Retry with backoff |
| Connection timeout | Transient | Retry with backoff |
| 400, 401, 403 | Permanent | Fail immediately |
| 404, 422 | Permanent | Fail immediately |

`ErrorOrigin` variants in the state machine: `Llm`, `Tool`, `Io`.

---

## Retry Strategy (Exponential Backoff)

| Provider | Base Delay | Backoff Factor | Max Delay | Max Attempts |
|----------|-----------|---------------|-----------|-------------|
| Anthropic | 200ms | 2× | 5s | 3 |
| OpenAI | 500ms | 2× | 30s | 3 |

**Jitter** is added to prevent thundering herd when concurrent failures retry simultaneously. Same provider is always retried; no cross-provider fallover is implemented.

---

## Error Paths

| Error Type | Classification | Recovery Action |
|-----------|---------------|----------------|
| LLM call failure (transient HTTP) | Transient | Exponential backoff retry; `ErrorOrigin::Llm` |
| LLM call failure (permanent HTTP) | Permanent | Transition to `WaitingForUserInput` with error |
| Retries exhausted | — | Return to `WaitingForUserInput`, display error |
| Malformed JSON / parse failure | Permanent | Transition to error state; fresh context on next iteration implicitly corrects |
| Rate limit (HTTP 429) | Transient | Backoff retry; OpenAI uses longer delays |
| Tool security violation (path traversal) | Permanent | `ToolError` with "Path outside workspace"; does not retry |
| Tool execution failure | Transient/Permanent | `ErrorOrigin::Tool`; different retry logic than LLM errors |
| Tool hanging / no response | Transient | Weaver handshake timeout: 10s; session TTL auto-cleans zombie connections |
| `cargo test` failure | — | `git_reset_hard()` reverts all uncommitted changes; loop restarts next iteration |
| Git operation failure | Permanent | `VcsError` propagates; not retried |
| Weaver pod crash | — | K8s detects non-zero exit → `WeaverStatus::Failed`; `reconcile_orphaned_weavers()` on restart |
| Server downtime mid-conversation | — | Loop fails; bash outer loop handles resumption on next iteration |
| Context window full | — | Prevention only (see below); no in-session recovery |
| `RALPH-BLOCKED.md` present | — | Loop exits immediately; no retry |
| I/O errors (filesystem, sockets) | Transient/Permanent | `ErrorOrigin::Io`; transient retried, permanent exits to error state |
| OOM / Rust panic | — | Crash analytics captures; agent loop behavior undefined |

---

## State Machine

States: `WaitingForUserInput` → `CallingLlm` → `ProcessingLlmResponse` → `ExecutingTools` → `WaitingForUserInput`.

Error state logic:
```
if retry_count < max_retries:
    stay in Error; wait for RetryTimeoutFired event
else:
    transition to WaitingForUserInput with error message
```

The state machine is **synchronous** (`handle_event()` returns immediately). All async I/O and retry timers are managed by the caller, keeping transitions testable in isolation.

---

## Context Exhaustion Prevention

Context window exhaustion is **prevented, not recovered from**. The Ralph Loop architecture makes in-session recovery unnecessary:

- Each loop iteration starts with a **fresh, empty context window**.
- All progress state lives **outside the LLM**: git commits, `task.md`, `review-feedback.txt`, test results.
- Previous failed attempts are **not in conversation history** — they are reverted via git reset.
- Full specs are re-injected each iteration (intentional redundancy prevents "context rot").
- Reviewer feedback is written to a file; the worker reads it at the start of the next iteration.

This also prevents compaction events where the LLM is forced into lossy context compression mid-task.

---

## RALPH-BLOCKED.md Pattern

When the agent determines it cannot make forward progress (missing information, impossible constraint, unresolvable dependency), it writes `RALPH-BLOCKED.md` to the working directory.

- The outer loop (`loop.sh` or equivalent) checks for this file after each iteration.
- If present, the loop **exits immediately** rather than retrying.
- Prevents infinite retry spirals on genuinely unsolvable tasks.
- Requires human intervention to unblock (read the file, resolve the issue, delete the file, restart the loop).

File location (goose): `.goose/ralph/RALPH-BLOCKED.md`. Loom uses a consistent analog.

---

## Graceful Shutdown

| Component | Behavior |
|-----------|---------|
| Agent process | `ShuttingDown` state exists for controlled exit; signal handling (SIGTERM) is not documented |
| Loom server | Does **not** force-terminate Weaver pods on shutdown; leaves them for cleanup on restart |
| Weaver pods | Left running through server downtime; `reconcile_orphaned_weavers()` cleans up on restart |
| Sessions | Expire after 30 days; hourly rollups preserve aggregate metrics indefinitely |

---

## Observability

- Structured logging via `tracing` crate; fields extracted, not interpolated into strings.
- `loom-secret` types auto-redact sensitive values in all log output.
- `loom_weavers_failed_total` Prometheus counter incremented on pod failure.
- `weaver.failed` webhook event triggered.
- Weaver HTTP error codes: 500 (failed to start), 504 (timeout), 502 (K8s unreachable).

**Health endpoint** (`/health`): component-level diagnostics with graceful degradation.

| Component | Failure Behavior |
|-----------|----------------|
| Database | HTTP 503 (unhealthy) — critical |
| LLM provider | HTTP 200 (degraded) — non-critical |
| Other components | Non-blocking; endpoint still responds |

---

## Under-Documented / Missing Error Paths

These 10 paths were not found in any spec or source:

1. **Max retry count** — specs say "bounded" but no specific number is stated.
2. **Cross-provider fallover** — no evidence of trying Anthropic if OpenAI exhausts retries.
3. **Tool runtime timeout** — Weaver handshake is 10s; maximum tool execution duration is unspecified.
4. **SIGTERM / signal handling** — agent process signal behavior not documented.
5. **OOM recovery** — crash analytics captures panics but agent loop behavior on OOM is undefined.
6. **WireGuard tunnel partition mid-execution** — what happens to an in-flight tool call if the tunnel severs.
7. **Prompt injection via tool stderr** — adversarial content in tool output handling not specified.
8. **Filesystem full** — behavior when workspace or `/tmp` fills during tool execution undefined.
9. **OAuth token expiry** — token refresh for OpenAI OAuth device flow not documented.
10. **Cascading health check failures** — whether agent loop degrades gracefully or hard-fails when database is down.

---

## Key Insights for ABA

**NEW: Jitter on retries is load-bearing.** Without jitter, concurrent agent failures synchronize their retries and hammer the provider simultaneously. ABA's current retry logic (if any) should add jitter.

**NEW: OpenAI needs 2.5× longer backoff than Anthropic.** The provider-specific delay difference (200ms vs 500ms base, 5s vs 30s max) reflects real observed rate limit behavior. ABA's `OpenAiOAuthClient` should use the longer schedule.

**The bash outer loop is the real recovery unit.** In-process retry handles transient LLM errors. Everything else (test failures, stuck agents, context exhaustion) is handled by the outer loop restarting with fresh state. ABA's `loop.sh` already follows this model.

**`RALPH-BLOCKED.md` is the human escalation interface.** Rather than alerting or crashing, the agent writes a structured file explaining the blockage and exits cleanly. This is a protocol ABA should adopt: define what a blocked state looks like and how the engineer unblocks it.

**State persistence is git, not memory.** The LLM's context window is ephemeral. The only durable state is what has been committed. ABA already uses `git_reset_hard()` on failure and `git_commit_all()` on success — this is architecturally correct.

**The health endpoint pattern applies to ABA's future server mode.** If ABA gains a daemon/server mode, non-critical component failures (one LLM provider down) should not return 503 — they should degrade gracefully and surface in a structured health response.

**Weaver orphan reconciliation on startup** is a pattern worth adopting for any stateful subprocess ABA launches. Clean up leftovers from previous crashes before starting new work.
