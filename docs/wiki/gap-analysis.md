# ABA Gap Analysis & Adaptation Roadmap

Reference wiki synthesizing the Loom gap analysis and HumanLayer patterns research.
Sources: `docs/research/loom-gap-analysis-vs-aba.md`, `docs/research/humanlayer-patterns-for-aba.md`

---

## Master Gap Table

All gaps grouped by priority tier. Milestone column uses ABA's milestone numbering where applicable; otherwise lists HumanLayer phase (HL-1 through HL-4).

### P0 — Bootstrap Blockers

| Feature | Loom Status | ABA Status | Milestone |
|---|---|---|---|
| Session persistence / resume | `loom-server-sessions` SQLite store with full turn history | Not implemented; agent is single-shot per invocation | M5 |
| Rollback spec on failed push | Autonomous pushes with auto-rollback in `loop.sh` | `loop.sh` can push; no documented rollback strategy | M4 |
| Full fitness check (test+lint+fmt) | Full verification suite (`TEST_PLAN.md`) | `cargo test` only in PostToolsHook; missing clippy+fmt | M1 |
| Subagent guidance in prompts | Loom prompts instruct agents to use subagents for parallel file study | Updated: PROMPT_plan.md and PROMPT_build.md now include subagent guidance | M2 |

### P1 — Near-Term Capability Gaps

| Feature | Loom Status | ABA Status | Milestone |
|---|---|---|---|
| Cost tracking (per-call logging) | Not detailed | Not implemented; LLM calls not instrumented | M4 |
| Budget enforcement (hard stop) | Per-provider rate limits in proxy | Soft limits specced; no enforcement mechanism | M5 |
| LLM-generated commit messages | Claude Haiku generates 5-7-5 haiku messages | Mentioned in M6 spec; not implemented | M6 |
| Multi-LLM model router | `loom-llm-anthropic`, `loom-llm-openai`, `loom-llm-vertex` | Anthropic + OpenAI supported; no Vertex, no router | M5 |
| Feature flags / kill switches | Runtime toggles; canary support | Not mentioned | M6 |
| FTS5 thread search | FTS5-based thread persistence across agents | M5 plans FTS5; no search tools for agents | M5 |

### P2 — Medium-Term Architecture Gaps

| Feature | Loom Status | ABA Status | Milestone |
|---|---|---|---|
| eBPF kernel observability | `loom-weaver-ebpf`: syscall/file I/O/network tracing | Mentioned as future consideration; not planned | M8+ |
| Fleet management / weavers | K8s ephemeral pods, health checks, resource limits, TTL cleanup | M8 mentions weavers; no lifecycle spec | M8 |
| Concurrent agent coordination | FTS5 thread DB synced across all agents | Thread DB planned; no multi-agent sync model | M8 |
| Resource limits per agent | CPU/memory/disk quotas per weaver | JJ workspaces for isolation; no quota enforcement | M8 |
| Real-time streaming (WS/SSE) | WebSocket/SSE for live agent output, metrics, log tailing | JSON thread files only; no real-time push | M10 |
| Web UI (Svelte 5) | Complete dashboard for monitoring and interaction | Planned M10; no specification | M10 |
| TUI components | `loom-tui-*`: markdown renderer, modals, scrollable areas | No TUI framework; M6 specifies CLI only | M6 |
| Audit trail (kernel-level) | `loom-weaver-audit` + SOPS + age secrets | Redaction specced; no kernel-level audit trail | M8+ |
| Work distribution (pull model) | Agents pull tasks from shared queue | `IMPLEMENTATION_PLAN.md` shared; push model only | M8 |

### P3 — Enterprise / Future Gaps

| Feature | Loom Status | ABA Status | Milestone |
|---|---|---|---|
| SCIM provisioning | `loom-auth-scim`: Okta, Entra ID auto-provisioning | Not planned | — |
| ABAC (attribute-based authz) | 193+ authz tests: orgs, teams, repos, webhooks | API key isolation only; no fine-grained ABAC | — |
| Webhook system | Event webhooks (commits, test failures, etc.) | Not mentioned | — |
| Crash reporting | `loom-crash-reporting`: collect and analyze agent failures | Not mentioned | — |
| Cron job execution | `loom-cron`: scheduled looping tasks | Not mentioned | — |
| cargo2nix reproducible builds | Per-crate Nix caching; deterministic CI | NixOS flake exists; no cargo2nix | — |
| PostHog-style analytics | Identity resolution, outcome metrics per session | Token tracking planned; no user-centric analytics | — |
| Version management (self-update) | `loom-common-versioning`: track version across fleet | No versioning strategy for self-updates | — |
| Role-based tool restriction | Git-only agents, DB-only agents, audit-only agents | General-purpose tools only; no per-agent selection | — |
| Per-model pricing tables | Configurable model-specific rates | Generic cost estimation; no per-model rates | — |
| Cost attribution | Link costs to features/repos/agents | Not mentioned | — |
| MCP tool provenance / allowlist | Allowlist + trusted-source warnings | Hardcoded tools; no MCP security model | M9+ |

---

## Gaps Grouped by Priority

### P0 — Fix Now (Bootstrap Path)

1. **Session persistence** — Single-shot invocations cannot survive long Ralph loops. File-based JSON sessions (~300 lines) enable resume-on-restart.
2. **Approval gate** — Required before ABA touches shared systems. Serialize agent state, pause, wait for signal, resume with feedback.
3. **ACE-FCA context engineering** — Research→Plan→Implement phases with human review gates dramatically improve success rate on complex tasks.
4. **Rollback spec** — Document the exact rollback strategy when `git push` fails in autonomous mode.
5. **Formal test strategy** — Replace ad-hoc `cargo test` with a documented test plan (`TEST_PLAN.md`) that ABA's own Ralph loop can verify.

### P1 — This Quarter

6. **Cost logging** — Instrument every LLM call: model, input_tokens, output_tokens, cost → `~/.config/ABA/cost_log.jsonl`.
7. **Budget hard stop** — Kill the loop gracefully when spend exceeds threshold; write session summary.
8. **LLM commit messages** — Use a cheap model (Haiku/Flash) to generate commit messages. Completes M6 spec item.
9. **Multi-LLM router** — Abstract model selection; add Vertex as third provider.
10. **Feature flags** — Runtime toggles for unsafe operations; needed before autonomous trunk pushes.

### P2 — Next Quarter

11. **Fleet lifecycle spec** — Document M8 weaver lifecycle: spawn, health check, TTL cleanup, resource limits.
12. **Real-time streaming** — WebSocket/SSE output layer; prerequisite for M10 dashboard.
13. **FTS5 search tools** — Agent-accessible search over past threads and commits.
14. **eBPF observability** — Kernel-level tracing for production safety; defer until M8+.

### P3 — Future / Enterprise

- SCIM, ABAC, webhooks, crash reporting, cargo2nix, PostHog analytics, cost attribution.

---

## HumanLayer Adaptation Roadmap

### Pattern 1: Approval Gate

**Problem:** Prevents dangerous operations (destructive git commands, deploys, `rm -rf`) without requiring full human oversight of every step.

**HumanLayer mechanism:** `require_approval` decorator wraps specific tool calls. Agent pauses, sends async notification (Slack/email), serializes its reasoning context, and resumes when approval arrives with optional human feedback embedded.

**ABA adaptation:**
- Add `ApprovalRequired` marker to tool definitions for `git push`, destructive filesystem ops
- On approval-needed tool call: serialize `AgentState { turn, messages, last_tool_result }` to disk, emit signal, block
- On human approval: reload state, inject feedback as tool result, resume agent loop
- Minimum viable: ~200 lines; no async needed initially (poll a file or stdin)

**ABA phase:** HL-3 (Phase 3, unattended operation). Not needed for local-only dev.

---

### Pattern 2: Session Persistence

**Problem:** Agents crash or are interrupted mid-task. Work repeats from scratch on restart.

**HumanLayer mechanism:** `hld` Go daemon with SQLite session store. Full conversation history, tool results, and agent state persist per session. CLI/Web/API can all resume the same session.

**ABA adaptation:**
- Save `SessionState { id, turn, messages, git_commit }` as JSON after each tool execution
- Store at `~/.config/ABA/sessions/{session_id}.json`
- Add `--resume {session_id}` CLI flag to reload and continue
- ~300 lines; file-based, no daemon initially

**ABA phase:** HL-2 (medium priority, useful for long Ralph loops now).

---

### Pattern 3: ACE-FCA Context Engineering

**Problem:** Large codebases overwhelm LLM context. Quality degrades without structured knowledge extraction. Frequent context resets waste tokens on re-discovery.

**HumanLayer mechanism — Frequent Intentional Compaction (FIC):**

Target **40-60% context window utilization** across three isolated phases:

| Phase | Agent Action | Output Artifact | Human Gate |
|---|---|---|---|
| **Research** | Explore codebase (file search, grep, trace) | `RESEARCH_OUTPUT.md` (~500 lines) | Review + annotate |
| **Plan** | Convert research + notes to implementation plan | `TASK_PLAN.md` (~200-300 lines) | Review step-by-step approach |
| **Implement** | Execute plan, edit files, test | Committed code + embedded plan spec | Final diff review |

Noisy operations (wide searches, analysis) run in separate context windows and return compact summaries. On verification failure, loop within the Plan phase only — not back to full re-research.

**ABA adaptation:**
- Phase 2 prompts: extend `PROMPT_build.md` to produce `CODEBASE_RESEARCH.md` as first artifact
- Add optional `--phase research|plan|implement` flag to `loop.sh`
- Human reviews artifacts between phases (existing manual step, not automated)
- Phase 4+: separate sub-agent contexts per phase with compacted handoffs

**ABA phase:** HL-2 (start) → HL-4 (multi-agent). Highest ROI pattern.

---

### Pattern 4: Outer Loop vs. Ralph Loop

| Aspect | Ralph Loop (ABA) | Outer Loop (HumanLayer) |
|---|---|---|
| Iteration trigger | `verifyCompletion()` check fails → retry | Time-based, event-based, or manual |
| Human involvement | Tool-agnostic (any check) | Gated on specific tools (approval gates) |
| Context reset | Each iteration from full context | Persistent session; approvals interrupt flow |
| Scope | Single task/objective | Multi-step workflows, extended operations |
| Async support | No (synchronous verification) | Yes (awaits human feedback asynchronously) |

**The Ralph loop is a synchronous, single-shot special case of the outer loop.** ABA should treat Phase 3 as the transition point where Ralph loops gain session persistence and approval gates, effectively becoming outer loops.

---

### Pattern 5: claudecode-go Sub-Agent Pattern

**Problem:** Spawning Claude Code as a subprocess for every sub-task wastes ~50K tokens loading global context repeatedly.

**HumanLayer mechanism:** `SessionConfig` + `MCPConfig` struct controls what the subprocess sees: system prompt, allowed tools, output format, max turns.

**ABA adaptation:** Not immediately applicable — ABA is the agent, not an orchestrator. Relevant in Phase 4+ if ABA spawns isolated sub-agents (research-only, implement-only). Prerequisite: session persistence (#2).

**ABA phase:** HL-4 only.

---

### Pattern 6: Cost Tracking

**ABA minimal approach:**
- Log per LLM call: `{ model, input_tokens, output_tokens, cost_usd, session_id, timestamp }`
- Append to `~/.config/ABA/cost_log.jsonl`
- Ralph loop accumulates total spend per iteration; graceful shutdown on budget exceeded
- `aba cost-report --since {date}` CLI command
- ~150 lines

**ABA phase:** HL-2 (basic logging) → HL-3 (budgets + alerts).

---

### Pattern 7: MCP Security (Tool Provenance)

**Problem:** MCP tool descriptions are injected into system prompts — a prompt injection vector. Malicious descriptions can redirect agent behavior.

**ABA adaptation (Phase 4+ only):**
- Tool registry with provenance: `aba:bash` (internal, trusted) vs. `mcp://external` (requires flag)
- Validate tool descriptions for suspicious imperative patterns on load
- Log all tools loaded per session
- `--allow-mcp` flag required to load external tools

**ABA phase:** HL-4 planning stage.

---

## NEW Insights (Not in Either Source Document)

These patterns emerge from reading both documents together:

**1. The ACE-FCA phases map directly onto ABA's existing `loop.sh` modes.**
`loop.sh plan` is already the Research+Plan phase. The missing piece is a third mode — `loop.sh implement` — that operates from a frozen `TASK_PLAN.md` without re-researching. This would be a ~20-line change to `loop.sh` and dramatically reduce context waste on complex tasks.

**2. Session persistence is the single highest-leverage unblocking change.**
Five other patterns (approval gates, outer loop async, sub-agents, cost attribution, budget enforcement) all require session state as a prerequisite. Implementing file-based sessions first unlocks the entire HL-2 through HL-4 roadmap.

**3. The approval gate pattern + ABA's existing git revert = a safe production path today.**
ABA already has `git_reset_hard()` on test failure. Adding an approval gate *before* `git push` (not before commit) requires almost no architectural change — just a blocking read on stdin or a file sentinel after the commit step in `loop.sh`. This could be a 10-line shell addition.

**4. Loom's crate modularity is a long-term forcing function.**
ABA's single-package structure will become a refactoring bottleneck at fleet scale. The correct time to extract crates is when a second binary or library consumer is needed — likely when a TUI or web server is added (M10). Plan for this split at M6 design time.

**5. FIC's 40-60% context utilization target is a concrete metric ABA lacks.**
ABA has no context health monitoring. Adding input token count to the agent loop's turn summary (already emitted as JSON) would enable `loop.sh` to break and restart an agent when context exceeds a threshold — preventing degradation without needing full FIC infrastructure.

---

## Priority Actions for ABA (Top 10)

Ordered by ROI × urgency. Each action is self-contained and unblocks something else.

| # | Action | Effort | Unblocks |
|---|---|---|---|
| 1 | **Extend `loop.sh` with `implement` mode** operating from frozen `TASK_PLAN.md` | ~20 lines shell | Context efficiency, FIC phases |
| 2 | **Add context utilization monitoring** — log input token count per turn; break loop at >60% threshold | ~50 lines Rust | Context health, prevents degradation |
| 3 | **Add approval gate to `loop.sh`** before `git push` — blocking stdin read or file sentinel | ~10 lines shell | Safe unattended operation |
| 4 | **Implement file-based session persistence** — serialize `AgentState` JSON after each tool call | ~300 lines Rust | Resume on crash, outer loop async, sub-agents |
| 5 | **Instrument LLM calls** — log `{ model, tokens_in, tokens_out, cost }` to `cost_log.jsonl` | ~150 lines Rust | Cost visibility, budget enforcement |
| 6 | **Write `TEST_PLAN.md`** — document what tests ABA runs on itself, what constitutes a passing iteration | ~1-2 hrs writing | Self-bootstrap quality, Loom parity |
| 7 | **Document rollback strategy** for failed `git push` in `loop.sh` — match Loom's auto-rollback model | ~30 lines shell | Production safety |
| 8 | **Plan M6 crate split** — identify what goes into `aba-core`, `aba-agent`, `aba-vcs` before M10 | Design doc | Future modularity |
| 9 | **Add `--budget-usd` flag** to agent/loop — hard stop when LLM spend exceeds threshold | ~100 lines Rust | Unattended cost safety |
| 10 | **Implement LLM-generated commit messages** using a cheap model (Haiku/Flash) | ~80 lines Rust | M6 spec completion, Loom parity |

---

## Key Blind Spots (Loom Top 5)

Ranked by severity for ABA's production use:

1. **Kernel-level observability (eBPF)** — No syscall/file I/O/network tracing. Agents can misbehave at OS level with no detection. Defer to M8+ but plan the architecture now.
2. **Fleet lifecycle management** — M8 mentions weavers but has no health check, TTL cleanup, or resource quota spec. Required before multi-agent deployment.
3. **Autonomous trunk push + rollback** — Specced but untested. Needs formal rollback documented and a test that validates it (a Ralph loop that breaks and recovers).
4. **Real-time streaming UI** — JSON thread files are not sufficient for monitoring unattended agents. WebSocket/SSE layer needed before M10 dashboard is useful.
5. **Enterprise auth (SCIM/ABAC)** — Only relevant if ABA becomes a team product. Defer unless roadmap changes.
