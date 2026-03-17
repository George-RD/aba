# Loom Architecture Reference

*Compiled from deep research into ghuntley/loom for ABA's self-improvement.*
*Date: 2026-03-17 | Sources: 19 parallel research agents analyzing Loom codebase, specs, and Geoffrey Huntley's public talks.*

---

## Overview

**Loom** is Geoffrey Huntley's production-grade AI-powered coding agent built in Rust. It implements the **Ralph Wiggum Loop** philosophy: instead of interactive AI coding (Cursor-style back-and-forth), program simple single-objective loops that iterate until a goal is achieved. Engineers act as managers orchestrating agent loops, not manually guiding individual agents.

**Scale**: 99+ Rust crates organized into functional domains (core, server, auth, LLM, tools, TUI, weaver, analytics), plus a Svelte 5 web frontend, NixOS infrastructure, and Kubernetes-based remote execution (Weavers).

**Core Philosophy**: "Progress persists in files and git, not in the LLM's context window." Each Ralph loop iteration starts with fresh context, reads the full specification, works toward the goal, runs tests, commits on success, and hard-resets on failure.

**Key Quote**: "It's not that hard -- 300 lines of code running in a loop with LLM tokens. You keep throwing tokens at the loop and you've got an agent."

---

## Core Architecture

### State Machine (7 States, 8 Events, 8 Actions)

Loom uses a **pure functional state machine** where `handle_event(state, event) -> action` performs zero I/O. The caller is responsible for all side effects.

#### States

| State | Purpose |
|-------|---------|
| **WaitingForUserInput** | Idle, awaiting user input or tool results |
| **CallingLlm** | Building and sending LLM inference request |
| **ProcessingLlmResponse** | Parsing response, identifying tool calls vs. text |
| **ExecutingTools** | Running agent-requested tools, collecting results |
| **PostToolsHook** | Auto-commit via Haiku, analytics, thread persistence |
| **Error** | Retry with exponential backoff, error classification |
| **ShuttingDown** | Clean termination, flush pending writes |

#### Events

| Event | Trigger |
|-------|---------|
| **UserInputReceived** | User provides input in REPL |
| **LlmResponseReceived** | LLM returns inference result |
| **ToolsRequested** | LLM response contains tool_use blocks |
| **ToolExecutionCompleted** | All requested tools finished |
| **PostToolsHookCompleted** | Auto-commit and cleanup done |
| **ErrorOccurred** | Exception in any state |
| **ShutdownRequested** | User exit, goal complete, or fatal error |
| **RetryTriggered** | Retry timer fires after backoff delay |

#### Actions

| Action | Effect (performed by caller) |
|--------|------------------------------|
| **SendLlmRequest** | HTTP POST to server-side LLM proxy |
| **ExecuteTools** | Run tool handlers, collect results |
| **DisplayMessage** | Show text to user |
| **RunPostToolsHook** | Auto-commit, analytics, thread save |
| **RetryWithBackoff** | Wait `base_delay * (2 ^ attempt)`, then retry |
| **Shutdown** | Flush state, close connections, exit |
| **WaitForInput** | Display prompt, await stdin |
| **Continue** | Proceed to next state without I/O |

#### Full Transition Table

| Current State | Event | Action | Next State |
|--------------|-------|--------|------------|
| WaitingForUserInput | UserInputReceived | SendLlmRequest | CallingLlm |
| CallingLlm | LlmResponseReceived | Continue | ProcessingLlmResponse |
| ProcessingLlmResponse | ToolsRequested | ExecuteTools | ExecutingTools |
| ProcessingLlmResponse | (text only) | DisplayMessage | WaitingForUserInput |
| ProcessingLlmResponse | (end_turn) | Shutdown | ShuttingDown |
| ExecutingTools | ToolExecutionCompleted | RunPostToolsHook | PostToolsHook |
| PostToolsHook | PostToolsHookCompleted | SendLlmRequest | CallingLlm |
| Any | ErrorOccurred (retryable) | RetryWithBackoff | Error |
| Error | RetryTriggered (under max) | SendLlmRequest | CallingLlm |
| Error | RetryTriggered (exhausted) | DisplayMessage | WaitingForUserInput |
| Any | ShutdownRequested | Shutdown | ShuttingDown |

### Conversation Model

- **Message**: `{ role: user|assistant|tool, content: Vec<ContentBlock> }`
- **ContentBlock**: `Text(String) | ToolUse { id, name, input } | ToolResult { tool_use_id, output, error }`
- **ToolCall**: `{ tool_name: String, parameters: JSON, id: String }`
- **ToolResult**: `{ tool_use_id: String, output: String, error: Option<String>, exit_code: Option<i32> }`
- **LlmRequest**: Builder pattern with messages, model, max_tokens, tools, temperature
- **LlmResponse**: Tool calls, usage tracking (input/output tokens), stop_reason

### AgentConfig

TOML + env var parametrization controlling: model selection, temperature, available tools, retry behavior (base_delay, multiplier, max_attempts), max tool turns per iteration (default: 50).

### Tool System (7 Tools)

#### Tool Catalog

| Tool | Mutating | Description |
|------|----------|-------------|
| **bash** | Yes | Shell execution. Timeout: 60s default, 300s max. Captures stdout/stderr. |
| **edit_file** | Yes | Exact string replacement (old_str -> new_str). Not regex-based. |
| **read_file** | No | Read file contents. 1MB default truncation. |
| **list_files** | No | Directory exploration. |
| **oracle** | No | LLM delegation -- call another LLM as a tool for research/planning. |
| **web_search** | No | Web search with configurable max_results. |
| **code_search** | No | Ripgrep-based pattern search with file type filters and context lines. |

#### Tool Registry Architecture

- Trait-based design in `loom-tools` crate
- Each tool has a `ToolDefinition` struct (name, description, input_schema, function)
- Tools registered in central registry, looked up by name at execution time
- **Mutating tools** (bash, edit_file) trigger PostToolsHook; read-only tools do not

#### Execution Flow

1. LLM generates ToolCall with tool name and parameters
2. Registry looks up tool by name
3. Tool function executes in isolated context
4. Results captured (stdout/stderr/exit code)
5. ToolResult fed back to LLM in next message turn
6. LLM decides: respond or call another tool
7. Max 50 tool turns per iteration
8. PostToolsHook runs after mutating tools complete

---

## LLM Proxy

### Server-Side Proxy Pattern

All API keys stay server-side. The CLI never touches raw provider credentials.

```
loom-cli --> HTTP/SSE --> loom-server (proxy) --> LLM Provider API
                              |
                        Adds API key,
                        rate limits,
                        tracks tokens,
                        audit logs
```

### Provider Integrations

| Provider | Crate | Notes |
|----------|-------|-------|
| **Anthropic** | `loom-server-llm-anthropic` | Messages API, SSE streaming, OAuth pool |
| **OpenAI** | `loom-server-llm-openai` | Chat Completions API, org ID support |
| **Google Vertex AI** | `loom-server-llm-vertex` | Vertex API integration |
| **Zai** | `loom-server-llm-zai` | Custom/additional provider |

### Core Types & Interfaces

- **`LlmClient` trait**: Unified interface across all providers (complete, stream methods)
- **`LlmService`**: Server-side orchestration with multi-provider config, default model substitution
- **`ProxyLlmClient`**: Client-side HTTP client in loom-cli with provider factory methods
- **`LlmEvent`**: Streaming events: TextDelta, ToolCallStart, ToolCallDelta, ToolCallEnd, Completed, Error
- **`LlmStream`**: Async stream wrapper for SSE events
- **`Usage`**: `{ input_tokens, output_tokens }` for cost tracking

### OAuth Pool Management (Anthropic Max)

- Multi-account distribution for Claude Pro/Max subscriptions
- Three-tier account status: **Available**, **CoolingDown** (2-hour default on 429), **Disabled**
- Selection strategies: round-robin, first-available
- Background token refresh task
- Error classification: transient (retry) / quota (cooldown) / permanent (disable)
- Health monitoring via `pool_status()` API

### Token Tracking

- `Usage` struct tracks input/output tokens per request
- Cost calculator with provider/model-specific pricing tables
- Analytics by provider, user, and model
- Daily budget enforcement with cost attribution

### Rate Limiting & Security

- `RateLimiter`: per-user/per-IP with time windows
- Token budget limits per request/session
- `PathSanitizer`: credential redaction in logs
- `QueryValidator`: payload inspection
- Prometheus metrics export (requests, tokens, errors, latency)

---

## User Interfaces

### CLI REPL (`loom-cli`)

**Commands**: `loom login`, `loom version`, `loom update`, `loom resume`, `loom search`, `loom list`, `loom private`, `loom new`, `loom attach`, `loom weaver ps`, `loom weaver delete`, `loom ssh`

**REPL Flow**: prompt -> SSE stream -> tool calls -> execution -> auto-commit -> repeat

**Modes**:
- Interactive: TUI with manual approval prompts and modal dialogs
- Autonomous: `--dangerously-skip-permissions` flag for auto-approval (Ralph loop mode)

### TUI (16 Widget Crates, Ratatui-Based)

| Crate | Widget |
|-------|--------|
| `loom-tui-app` | Main TUI application |
| `loom-tui-core` | Core framework |
| `loom-tui-component` | Component system |
| `loom-tui-theme` | Theming support |
| `loom-tui-testing` | TUI testing utilities |
| `loom-tui-storybook` | Component showcase |
| `loom-tui-widget-header` | Header bar |
| `loom-tui-widget-input-box` | Multi-line input |
| `loom-tui-widget-markdown` | Markdown renderer |
| `loom-tui-widget-message-list` | Chat/message list |
| `loom-tui-widget-modal` | Modal dialogs |
| `loom-tui-widget-scrollable` | Scrollable content |
| `loom-tui-widget-spinner` | Loading spinners |
| `loom-tui-widget-status-bar` | Status bar with token counts |
| `loom-tui-widget-thread-list` | Thread list with search |
| `loom-tui-widget-tool-panel` | Tool execution status |

### Web (Svelte 5)

- **Stack**: SvelteKit + Tailwind CSS + Vite
- **Features**: Real-time SSE streaming, code syntax highlighting, diff viewer, agent control panel, session management, OAuth flow handling
- **Svelte 5 Runes**: Uses `$state()`, `$derived()`, `$effect()`, `$props()` (NOT Svelte 4 patterns)
- **Supporting packages**: analytics, crash, crons, flags, http
- **i18n**: Lingui with multi-language support (en, es, ar with RTL)

### Agent Client Protocol (ACP)

- JSON-RPC 2.0 over stdio
- Supports Zed, VSCode, Neovim, JetBrains IDEs
- Standard for editor-agent integration

---

## Thread Persistence

### Data Model

**Thread ID**: UUID7 with `T-` prefix (e.g., `T-01234567-89ab-cdef-0123-456789abcdef`)

**Core Fields**:

| Field | Type | Purpose |
|-------|------|---------|
| `thread_id` | UUID7 (T- prefix) | Primary identifier |
| `version` | i64 | Optimistic concurrency control |
| `conversation` | `Vec<Message>` | Full message history |
| `workspace` | WorkspaceSnapshot | Repo, branch, model context |
| `agent_state` | AgentStateSnapshot | Current execution state |
| `title` | String | Human-readable title |
| `tags` | `Vec<String>` | Categorization tags |
| `message_count` | u32 | Conversation length |
| `is_private` | bool | If true, NEVER syncs to server |
| `visibility` | Enum | Organization / Private / Public |
| `created_at` | Timestamp | Creation time |
| `updated_at` | Timestamp | Last modification |
| `last_activity_at` | Timestamp | For recency ranking |
| `metadata` | JSON | Flexible custom fields |

### Local Storage (XDG, JSON)

- **Data path**: `$XDG_DATA_HOME/loom/threads/{T-uuid7}.json`
- **Sync state**: `$XDG_STATE_HOME/loom/sync/pending.json`
- **Atomic writes**: Write to temp file, then atomic rename (prevents corruption)
- **Private threads**: `is_private=true` threads stay exclusively in local storage

### Server Sync (Offline-First)

**SyncingThreadStore** wraps LocalThreadStore with background sync:

```
Thread Operation
    |
Save to LocalThreadStore
    |
is_private=true?
    |-- YES --> Stop (never sync)
    |-- NO  --> Queue for background sync
                    |
               Try async sync to server
               |-- Success --> Remove from queue
               |-- Failure --> Queue in PendingSyncStore
               |               Retry with exponential backoff
```

**Sync triggers**: After inferencing (return to WaitingForUserInput), graceful shutdown (SIGINT, EOF, exit command).

**Optimistic concurrency**: Uses `If-Match` version headers. HTTP 409 on stale version.

**REST API**: `/v1/threads/` -- GET, POST/PUT (upsert), DELETE, search

### FTS5 Search

**Server-side SQLite** with WAL mode, `sqlx` 0.8:

```sql
CREATE VIRTUAL TABLE thread_fts USING fts5(
  thread_id UNINDEXED,
  title,
  content,        -- full message text
  git_commit,
  git_branch,
  git_repo_url,
  tags,
  tokenize = 'unicode61'
);
```

- **Ranking**: BM25 relevance score, tiebreaker on `last_activity_at`
- **Index sync**: Three database triggers (INSERT, UPDATE, DELETE) maintain FTS index
- **Migrations**: Centralized in `crates/loom-server/migrations/` as numbered SQL files

---

## Weaver System

### What is a Weaver?

An ephemeral Kubernetes pod running a Loom REPL for remote code execution and interactive agent sessions. Weavers are the distributed execution environment -- agents spin up, perform work, and tear down, all managed by Kubernetes as the source of truth.

### Pod Lifecycle

**Creation flow**: `loom new` -> CLI POST `/api/weaver` -> loom-server-k8s provisions pod -> WebSocket attachment

**Pod specification**:
- **Naming**: `weaver-{uuid7}` in `loom-weavers` namespace
- **Workspace**: Git repo cloned to `/workspace`
- **Resources**: 16GB memory default, ephemeral volumes only
- **TTL**: Default 4 hours, max 48 hours override
- **Labels**: `app: loom-weaver`, `weaver-id`, `owner`, `created-at`

**Cleanup**: Background task checks `creation_time + ttl < now()`, deletes expired pods. Orphan reconciliation on server restart via `reconcile_orphaned_weavers()`.

### Security Hardening

```yaml
securityContext:
  runAsNonRoot: true
  readOnlyRootFilesystem: true
  allowPrivilegeEscalation: false
  capabilities:
    drop: [ALL]
    add: []
```

- Non-root execution (UID > 1000)
- All Linux capabilities dropped
- Read-only root filesystem (writable: `/workspace`, `/tmp` only)
- Complies with Kubernetes `restricted` Pod Security Standard
- No custom service accounts or mounts allowed

### eBPF Audit Sidecar

**Crates**: `loom-weaver-audit-sidecar`, `loom-weaver-ebpf-common`, `loom-weaver-ebpf`

- Kernel-level syscall tracking (file I/O, network activity, resource consumption)
- Real-time CPU, memory, file descriptor monitoring per agent process
- All operations logged with weaver ID for audit trail
- `loom_weavers_failed_total` Prometheus counter

### WireGuard + DERP

**Principle**: "P2P when possible, relay when needed"

- **WireGuard tunnels**: Direct encrypted connection between user device and weaver pod
- **DERP relay fallback**: Routes through encrypted relay servers when direct P2P blocked (NAT/firewall)
- **Path upgrade**: Continuously attempts upgrade from DERP to direct (30s polling)
- **Key exchange**: loom-server coordinates; never sees encrypted traffic
- **SPIFFE credentials**: Weaver pods authenticate using SPIFFE SVID

**Crates**: `loom-wgtunnel-common`, `loom-wgtunnel-conn`, `loom-wgtunnel-derp`, `loom-wgtunnel-engine`

```bash
loom ssh <weaver-id>  # Auto-establishes WireGuard tunnel + SSH
```

### SPIFFE Identity + Secrets

- Pods receive SPIFFE SVID (Service Identity) for authentication
- `loom-weaver-secrets`: Client library for accessing secrets from within pods
- All LLM calls go through loom-server proxy (pods never hold raw API keys)
- File-based secret injection for Kubernetes

---

## Observability

### Analytics (PostHog-Style, Self-Hosted)

- Event capture (single and batch)
- Identity resolution (anonymous -> identified, alias, person merge)
- Property tracking with PII redaction
- Write-only API keys for SDK authentication
- **Rust SDK** (`loom-analytics`): async HTTP with batching, offline queue, circuit breaker
- **TypeScript SDK** (`@loom/analytics`): browser-safe queueing, Service Worker integration

### Crash Tracking

- Stack trace symbolication and deduplication
- Crash-free rate metrics
- Breadcrumb trail capture (50 log lines before crash)
- Auto-grouping by fingerprint
- Issue lifecycle: Unresolved -> Resolved -> Regressed
- Crashed sessions always stored (never sampled out)

### Session Health

- Session lifecycle tracking with health grading (A-D based on 90-50% thresholds)
- Real-time event rate monitoring
- Tool success rate and cost efficiency tracking
- 30-day retention with hourly rollups

### Cron Monitoring

- Missed run detection
- Timeout and performance degradation tracking
- Multi-level escalation alerting
- Success rate and execution history

### Health Endpoint (`/health`)

- 200 OK public endpoint (no auth required)
- Component-level health: API, database, LLM providers, analytics, cache
- Cascading failure detection
- Check intervals: fast (10s), medium (30s), slow (5m)
- Each subsystem implements `HealthCheck` trait

### Cost Tracking

- `Usage` struct: `{ input_tokens, output_tokens, cost_usd }`
- Per-provider pricing tables (Anthropic, OpenAI, Vertex)
- Cost aggregation by agent/session/user
- Token efficiency metrics
- Daily budget enforcement

### Structured Logging

- JSON output with `tracing` crate
- Contextual span tracking: `session_id`, `goal`, `turn`
- `#[instrument(skip(self, secrets, large_args), fields(id = %id))]` pattern
- NEVER log secrets directly; use `loom-secret::SecretString`
- Log levels: TRACE, DEBUG, INFO, WARN, ERROR

### Self-Healing Ralph Loop

- Monitor -> Detect -> Analyze -> Fix -> Deploy -> Verify cycle
- **January 2026 Achievement**: First autonomous bug identification, fix, deployment, and verification (20 minutes, zero human intervention)
- Retry on failure, escalate after N attempts

---

## VCS & Auto-Commit

### Git Integration

**`loom-git` crate** with `GitClient` trait:

| Method | Purpose |
|--------|---------|
| `diff_staged()` | Show staged changes |
| `diff_unstaged()` | Show unstaged changes |
| `changed_files()` | List modified files |
| `stage(paths)` | Stage files for commit |
| `commit(message)` | Create commit |
| `status()` | Query repo status |

Implementations: `CommandGitClient` (real git CLI), `MockGitClient` (test double with builder pattern).

### Spool (JJ-Based VCS)

Spool is Loom's integrated version control, built on Jujutsu (jj) with a textile metaphor:

| Git Concept | Spool Term | Description |
|-------------|-----------|-------------|
| Commit | **Knot** | Stitch with a message |
| Anonymous change | **Stitch** | Atomic work unit (WIP) |
| Working directory | **Shuttle** | Current workspace |
| Rebase | **Rethreading** | Auto-rebase on pull |
| Merge conflict | **Tangle** | Conflict resolution |
| Operation log | **Tension Log** | Full undo/audit trail |
| Checkout | **Draw** | Switch workspace |

**Crates**: `loom-common-spool` (library), `loom-cli-spool` (CLI), `loom-server-spool` (planned remote hosting)

**Key features**: Anonymous work-in-progress changes, automatic rebasing, conflict-aware operations, complete operation log with undo capability, Git colocated mode (`.spool/` and `.git/` synchronized).

**Tension Log**: Every VCS operation recorded with unique ID and timestamp. Contains "view objects" (full repo state snapshots). Enables time-travel queries and complete audit trails for agents.

### PostToolsHook Auto-Commit via Haiku

**Flow**:
1. Tools complete in ExecutingTools state
2. Mutation detected via `changed_files()`
3. Transition to PostToolsHook
4. Generate commit message via **Claude Haiku** (fast, cheap)
5. Stage and commit changes
6. PostToolsHookCompleted event fires
7. Resume CallingLlm state

**Commit message format**: Conventional commits (`<type>(<scope>): <description>`)
- Types: feat, fix, refactor, test, docs, chore
- Scope inferred from file paths
- Description < 72 characters
- Fallback: `"chore: auto-commit from loom [LLM unavailable]"`

**Constraints**:
- 32KB diff truncation limit (with notice in commit message)
- Fail-open strategy: never blocks agent execution
- Configurable via `LOOM_AUTO_COMMIT_DISABLE` env var
- `AutoCommitResult`: `{ committed, commit_hash, message, files_changed, truncated }`

### Back-Pressure Philosophy

- Agents push directly to trunk without human code review
- Safety comes from automated checks (tests, lints, builds), not humans
- No pull requests, no branch protection, no code review
- Every 10 seconds, NixOS auto-update checks trunk for new commits and auto-deploys
- On test failure: immediate revert + agent re-loops automatically
- "Multiple test-driven iterations converge to correct solutions faster than single human review"

---

## Infrastructure

### Nix Flake + cargo2nix

- **cargo2nix**: Reproducible per-crate builds across 99+ Rust crates
- Build commands: `nix build .#loom-cli-c2n`, `nix build .#loom-server-c2n`
- Per-crate caching: changed dependencies only recompile affected crates
- `Cargo.nix` must be regenerated (`cargo2nix-update`) when `Cargo.lock` or migrations change
- Development shell (`devShell`): Rust toolchain, Node.js, build tools, watchexec, ripgrep

### NixOS Auto-Update (10-Second Cycle)

**Mechanism** (systemd service `nixos-auto-update.service`):

```
git push trunk
  |
(NixOS server polls every 10 sec)
  |
Detects new commit (compares to /var/lib/nixos-auto-update/deployed-revision)
  |
nix flake update && cargo2nix-update
  |
nixos-rebuild switch (atomic)
  |
systemd service restart (loom-server.service)
  |
journalctl -u loom-server.service -f
```

- On success: update deployed-revision marker
- On failure: retain previous revision (safe rollback)
- Force rebuild: `sudo rm /var/lib/nixos-auto-update/deployed-revision && sudo systemctl restart nixos-auto-update.service`

### Deployment Flow

- No PRs, no merge queues -- push to `trunk` triggers auto-deploy
- Atomic NixOS deployments with full system rollback on failure
- Server at `/var/lib/depot`
- Verify: check deployed revision, loom-server restart time, `/health` endpoint

### Self-Update Mechanism

- `loom update` command
- `/bin/{platform}` endpoint for binary distribution
- NixOS flake integration for system-level updates

### CI/CD (GitHub Actions)

| Workflow | Purpose | Trigger |
|----------|---------|---------|
| `ci.yml` | Format, clippy, tests, Nix builds, security audits | Push to trunk, PRs |
| `build.yml` | Cross-platform binaries, container images | Push to trunk, PRs, releases |
| `update-devenv.yml` | Dependency update PRs | Monday 5am UTC |
| `ampcode-image.yml` | Ubuntu container with Ampcode | Every 6 hours |
| `publish-audit-sidecar.yml` | eBPF audit sidecar, cosign signing | Path changes, releases |

### Self-Healing Agent Workflow

`.agents/workflows/watch-nixos-auto-update.md`:
- Single persistent subagent runs every minute
- Monitors journald logs for cargo2nix deployment failures
- On failure: `git pull trunk` -> regenerate cargo2nix -> commit/push fix
- Uses single subagent throughout cycle for state consistency

---

## Security & Auth

### Authentication Providers (5)

| Method | Use Case | Details |
|--------|----------|---------|
| **OAuth 2.0** | Web login | GitHub, Google, Okta providers. Auto-links accounts by email. |
| **Magic Links** | Passwordless email | 10-minute expiry, single-use tokens |
| **Device Code Flow** | CLI/headless | 5s polling interval, no localhost binding needed |
| **Session Cookies** | Web browsers | 60-day sliding expiry, Secure+HttpOnly+SameSite:Strict |
| **API Keys** | Programmatic access | Scoped permissions, Argon2 hashing, rotation/revocation |

### ABAC Authorization

Fine-grained **Attribute-Based Access Control** with 193+ authorization tests:

- **Subject attributes**: user ID, org memberships, team memberships, global roles
- **Resource attributes**: owner, organization, team, visibility level, support-access flags
- **Actions**: Read, Write, Delete, Share, UseTool, UseLlm, ManageOrg
- **Thread visibility hierarchy**: Private -> Team -> Organization -> Public
- **Router pattern**: `PublicRouter` (unauthenticated), `AuthedRouter` (protected)

### Secret Management

- **Type-safe**: `Secret<T>` wrapper with compile-time redaction prevention
- **Memory**: Zeroization on drop
- **Hashing**: Argon2 for token storage
- **Encryption**: Envelope encryption (KEK/DEK pattern)
- **Kubernetes**: File-based secret injection
- **Repo secrets**: SOPS + age encryption
- **Logging**: `loom-secret::SecretString` -- access via `.expose()`, always `skip` in instrumentation

### Redaction System

- `loom-redact` crate: real-time secret detection using gitleaks patterns
- Prevents accidental exposure in logs, responses, and analytics

### Audit System

- Non-blocking fan-out architecture with bounded MPSC queue
- Enrichment pipeline with severity levels (RFC 5424)
- SIEM integration (Splunk, Datadog, Elastic)
- Graceful degradation -- never blocks request handling

### Feature Flags & Kill Switches

- Two-tier: platform-level and org-level
- Real-time SSE updates to clients
- Murmur3 hashing for consistent bucketing
- A/B testing support
- Runtime toggles to disable unsafe operations or rollback features

---

## Error Handling

### Retry with Backoff

| Parameter | Default (Anthropic) | Default (OpenAI) |
|-----------|-------------------|-----------------|
| Base delay | 200ms | 500ms |
| Backoff factor | 2x | 2x |
| Max delay | 5s | 30s |
| Max attempts | 3 | 3 |
| Jitter | Yes (prevents thundering herd) | Yes |

### Error Classification

| HTTP Status | Classification | Action |
|-------------|---------------|--------|
| 408, 429, 500, 502, 503, 504 | **Transient** (retryable) | Exponential backoff retry |
| 400, 401, 403, 404, 422 | **Permanent** (non-retryable) | Display error, stop |
| Timeout, connection failure | **Transient** | Retry |

**Error origins**: `Llm`, `Tool`, `Io` -- each with different retry logic.

### Context Exhaustion Prevention

Loom prevents context exhaustion rather than recovering from it:

- **Fresh context per iteration**: Each Ralph loop starts with empty context window
- **Progress in files, not context**: Task state persists in filesystem and git
- **No accumulation**: Previous failed attempts NOT in conversation history
- **Optimal window**: 40-60% context utilization recommended
- **Quality clipping**: Claude's 200k tokens -- quality degrades at 147-152k
- **Tool call failures**: Occur at compaction point

### RALPH-BLOCKED.md Pattern

- If this file exists, the Ralph loop exits instead of retrying
- Prevents infinite retry spirals on fundamentally blocked tasks
- Agent can write this file to signal "I need human help"

### Additional Safety Mechanisms

- **Bounded retry attempts**: State machine has `max_retries` threshold
- **Iteration cap**: `loop.sh` supports max iterations (e.g., `./loop.sh 20`)
- **Reviewer feedback loop**: If reviewer marks "REVISE," worker gets `review-feedback.txt` next iteration
- **Tool path validation**: Prevents directory traversal, workspace sandboxing
- **Crash analytics**: Panic hook integration, fingerprint deduplication, regression detection

---

## Geoffrey's Philosophy

### Key Quotes and Principles

> "While in San Francisco everyone is trying multi-agent communication multiplexing. At this stage, it's not needed. Consider what microservices would look like if the microservices themselves are non-deterministic -- a red hot mess."

> "Software development (typing code, prompting LLMs) is effectively dead and commoditized. Software engineering involves designing safe, reliable systems -- preventing failure scenarios through back pressure."

> "LLMs are literal-minded pair programmers excelling when given explicit, detailed instructions."

> "LLMs are mirrors of operator skill."

> "If the bowling ball is in the gutter, there's no saving it." (on death spirals)

> "The future belongs to people who can just do things."

### Five Core Primitives for a Coding Agent

1. **Read** tool (file contents)
2. **List** tool (directory exploration)
3. **Bash** tool (system commands)
4. **Edit** tool (file modification)
5. **Search** tool (ripgrep for pattern matching)

### Failure Modes and Solutions

| Failure Mode | Description | Solution |
|-------------|-------------|----------|
| **Context Rot** | LLM loses track of specs as context fills. Enters "dumb zone." | Fresh context each iteration. Full spec reloaded. |
| **Death Spiral** | Bad output leads to brute-force fixing on main context window. | Kill iteration, start fresh. Don't try to save it. |
| **Tool Failures** | Ripgrep non-determinism. Agent falsely concludes code is missing. | Instruct agents to verify, not assume. |
| **Compaction Loss** | Sliding window summarizes older content, losing critical context. | Avoid compaction entirely via Ralph loop. |

### Monolithic vs Multi-Agent

- Ralph operates as a **monolithic, single-process orchestrator** -- not a multi-agent system
- One task per loop iteration in a single repository
- Multi-agent complexity explicitly rejected at current stage
- Future: hierarchical subagent systems where agents spawn specialized workers with cloned context

### Context Window Management

- **Optimal utilization**: 40-60% of window
- **Quality cliff**: Claude's 200k tokens degrades at 147-152k
- **Tool call invocations fail at compaction point**
- **Solution**: Rotate to fresh context before pollution builds
- **Spec-driven development**: Clear specifications, technical plans, focused single tasks, declarative markdown specs

### Cost Data

- Running Claude Sonnet 4.5 on a Ralph loop: **$10.42 USD per hour**
- Target: **< $1.50 per feature**
- CURSED programming language (3 months of Ralph loops): $14,000 USD across three implementations (C, Rust, Zig) with compiler, standard library, and editor extensions

### Loom Vision (Long-Term)

- Infrastructure for evolutionary software -- in Huntley's head for 3+ years
- Includes cloned versions of GitHub and Daytona for complete source-to-execution control
- **Level 9 target**: Autonomous loops evolving products and optimizing automatically for revenue generation
- Related concepts: Gas Town (Steve Yegge, parallel agent orchestration), MEOW (Molecular Expression of Work, granular task handoff)

---

## Gap Analysis: Loom vs ABA

### What ABA Has

| Feature | ABA Status |
|---------|-----------|
| Multi-turn agent loop | Implemented (agent.rs, max 50 tool turns) |
| LLM abstraction | Implemented (Anthropic + OpenAI via trait) |
| Bash tool | Implemented |
| VCS trait | Implemented (GitVcs with commit_all, revert, status) |
| PostToolsHook | Implemented (cargo test -> commit/revert) |
| Ralph loop | Implemented (loop.sh with plan/build modes) |
| TOML config | Implemented (directories crate, XDG paths) |
| Nix flake | Implemented |
| SOPS + age | Implemented |

### What ABA Is Missing

| Gap | Loom Feature | Priority |
|-----|-------------|----------|
| **Pure state machine** | 7-state FSM with explicit events/actions, zero I/O in handler | High |
| **Server-side LLM proxy** | API keys never leave server, centralized rate limiting | Medium |
| **Auto-commit messages** | Claude Haiku generates conventional commit messages | High |
| **Thread persistence** | UUID7-based threads, JSON local storage, FTS5 search | Medium |
| **Multiple tools** | edit_file, read_file, list_files, oracle, web_search, code_search | High |
| **SSE streaming** | Real-time response streaming from LLM | Medium |
| **Error classification** | Transient vs permanent, per-provider retry config | High |
| **Context exhaustion prevention** | RALPH-BLOCKED.md, fresh context per iteration | Medium |
| **TUI** | 16 widget crates with markdown, modals, spinners | Low |
| **Web UI** | Svelte 5 dashboard with SSE streaming | Low |
| **Weaver system** | K8s ephemeral pods for remote execution | Low |
| **eBPF audit** | Kernel-level syscall tracking | Low |
| **WireGuard tunnels** | P2P + DERP relay for pod access | Low |
| **ABAC authorization** | Fine-grained attribute-based access control | Low |
| **Feature flags** | Runtime toggles, kill switches | Low |
| **Analytics** | PostHog-style event capture, crash tracking | Low |
| **Spool (JJ)** | Jujutsu-based VCS with textile semantics | Medium |
| **Multi-provider LLM** | Vertex AI, Zai, provider failover | Low |
| **Session persistence** | Resume interrupted sessions | Medium |
| **Cost tracking** | Per-request token/cost logging, budget enforcement | Medium |

### Priority Items for ABA

1. **Pure state machine refactor**: Move from procedural agent loop to explicit state/event/action model. Enables property-based testing and deterministic behavior.
2. **Additional tools**: Add read_file, edit_file (exact string match), list_files, code_search. The oracle tool (LLM delegation) is a powerful pattern for sub-agent work.
3. **Auto-commit message generation**: Use a small model (Haiku) to generate conventional commit messages. 32KB diff truncation, fail-open strategy.
4. **Error classification and retry**: Separate transient from permanent errors. Different retry configs per provider.
5. **Context exhaustion prevention**: Implement RALPH-BLOCKED.md pattern. Consider iteration-level context budget tracking.
6. **Thread/session persistence**: Store conversation state for resume capability. File-based initially, SQLite later.
7. **Cost tracking**: Log tokens and cost per request. Budget enforcement.
8. **Spool/JJ migration**: Replace GitVcs with JJ-based implementation. Operation log gives agents complete recovery capability.

---

## HumanLayer Patterns for ABA

### Approval Gates

**Pattern**: Wrap dangerous tool calls (git push, rm -rf, deploy) with approval decorators that pause execution until human approves/denies via Slack or email.

**ABA adaptation**:
- Add `ApprovalRequired` trait to tool definitions
- On approval-needed tool: serialize agent state, pause, wait for signal
- Resume with human feedback in context
- Start simple: only gate on git push and destructive filesystem ops
- Priority: Not needed now (ABA is local-only), critical when running unattended

### Session Persistence

**Pattern**: Central session store (SQLite) persisting full LLM conversation history, tool results, and agent state. Sessions resume from exact point on restart.

**ABA adaptation**:
- `SessionState { turn, messages, last_tool_result, git_status }`
- Save to `~/.config/ABA/sessions/{session_id}.json` after each tool execution
- On restart with `--session-id`, load state and resume
- File-based initially, not database
- Complexity: ~300 lines

### Context Engineering (ACE-FCA)

**Frequent Intentional Compaction** workflow targeting 40-60% context utilization:

1. **Research Phase**: AI explores codebase, produces ~500-line `RESEARCH_OUTPUT.md`
2. **Plan Phase**: AI converts research into `IMPLEMENTATION_PLAN.md` (200-300 lines, ~15-20% of fresh context)
3. **Implement Phase**: AI executes plan, tests, commits

**ABA already does this** via plan/build modes in `loop.sh`. Enhancement: add explicit research phase with compact output artifacts.

### Outer-Loop Concept

**Definition**: Agents operate autonomously, actively working toward goals. Communication between humans and agents is agent-initiated, triggered only when a critical function requires approval.

**vs. Ralph Loop**:

| Aspect | Ralph Loop | Outer Loop (HumanLayer) |
|--------|-----------|------------------------|
| Iteration trigger | `verifyCompletion()` fails -> retry | Time-based, event-based, or manual |
| Human involvement | Post-hoc (review commits) | Pre-hoc (approval gates on specific tools) |
| Context management | Fresh each iteration | Persistent with compaction |
| Failure recovery | Hard reset + re-loop | Pause + resume with feedback |

**ABA synthesis**: Ralph loop for the build cycle (fresh context, hard reset on failure), HumanLayer-style approval gates for dangerous operations (deploy, push to shared repos).

---

## AGENTS.md Conventions (from Loom)

Key patterns from Loom's AGENTS.md that ABA could adopt:

- **Specs first**: Always consult specs before implementing. Specs describe intent; code describes reality.
- **No comments**: Only comment complex code. Always add copyright headers.
- **Type safety**: Custom `Result<T>` aliases, `thiserror` for error enums, `anyhow` for propagation.
- **Secrets protection**: `SecretString` wrapper, skip in logging/instrumentation.
- **Testing**: Prefer property-based tests (`proptest`) over unit tests. 193+ authorization tests.
- **Structured logging**: `tracing` crate, never log secrets, `#[instrument(skip(secrets))]`.
- **HTTP clients**: Never build `reqwest::Client` directly; use shared builder for consistent User-Agent and retry.
- **Formatting**: Hard tabs, 2-space width, 100 char max line width.
- **Shared services**: When multiple code paths do similar things with slight variations, create a shared service with a request struct that captures variations.
- **Cargo2nix**: Must update and commit after Cargo.lock changes.
- **Deploy verification**: Study journald logs BEFORE and AFTER deployment.

---

## Complete Crate Inventory (99+ Crates)

### By Category

| Category | Count | Key Crates |
|----------|-------|-----------|
| **Core** | 3 | loom-core, loom-server, loom-cli |
| **Common Libraries** | 9 | loom-common-{core, config, http, i18n, secret, spool, thread, version, webhook} |
| **CLI Tools** | 9 | loom-cli-{acp, auto-commit, config, credentials, git, spool, tools, wgtunnel} |
| **Server Components** | 45+ | auth (5), llm (6), data/infra (12), integration (8), observability (5) |
| **TUI Widgets** | 16 | loom-tui-{app, core, component, theme, testing, storybook, widget-*} |
| **Analytics** | 2 | loom-analytics-core, loom-analytics |
| **Crash** | 3 | loom-crash-core, loom-crash-symbolicate, loom-crash |
| **Crons** | 2 | loom-crons-core, loom-crons |
| **Feature Flags** | 2 | loom-flags-core, loom-flags |
| **Security** | 2 | loom-redact, loom-scim |
| **Weaver/eBPF** | 5 | loom-weaver-{audit-sidecar, ebpf-common, ebpf, secrets} |
| **WireGuard** | 4 | loom-wgtunnel-{common, conn, derp, engine} |
| **Other** | 3 | loom-jobs, loom-sessions-core, loom-whatsapp |

### Specification Documents (67 Total)

Key specs organized by domain:
- **Architecture**: architecture.md, container-system.md, configuration-system.md
- **Auth**: auth-abac-system.md, claude-subscription-auth.md, anthropic-oauth-pool.md
- **Data**: analytics-system.md, audit-system.md, crash-system.md, sessions-system.md, thread-system.md
- **Integration**: github-app-system.md, llm-client.md, mcp-system.md, whatsapp-system.md, web-search-system.md
- **Operations**: crons-system.md, job-scheduler-system.md, auto-commit-system.md, feature-flags-system.md
- **Security**: secret-system.md, redact-system.md, scim-system.md, weaver-secrets-system.md, weaver-ebpf-audit.md
- **UI**: loom-web.md, vscode-extension.md, tui-system.md, design-system.md, observability-ui.md
