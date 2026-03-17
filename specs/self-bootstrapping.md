# ABA Self-Bootstrapping Roadmap

## Current State

ABA is a Rust CLI that implements the core Ralph Wiggum Loop. It reads a prompt from stdin, runs a multi-turn LLM conversation with a bash tool, then runs `cargo test` as a fitness check -- committing on pass, reverting on fail. The outer loop (`loop.sh`) feeds prompts repeatedly in plan or build mode. Auth supports Anthropic and OpenAI via direct API keys or OAuth. An nginx proxy architecture is specified for VPS deployment with privilege-separated key management.

What exists and works:
- Multi-turn agent loop with bash tool (`agent.rs`)
- LLM abstraction with Anthropic and OpenAI backends (`llm.rs`)
- VCS trait with GitVcs implementation (`tools.rs`)
- Config management with interactive auth setup (`config.rs`)
- Ralph loop infrastructure (`loop.sh`, `PROMPT_plan.md`, `PROMPT_build.md`)
- NixOS deployment config with nginx proxy and SOPS secrets
- Specs defining the target architecture

What does not exist yet:
- Conversational REPL (user cannot talk to ABA interactively)
- Additional tools beyond bash (read_file, edit_file, code_search)
- JJ VCS backend
- Thread persistence or search
- Structured logging or cost tracking
- Multi-agent support

## Milestone 0: Talk to ABA (Bootstrap Entry Point)

**Goal:** User can SSH in and have a conversation with ABA.

The conversational REPL comes first. Before ABA can do anything useful -- before it loops, before it plans, before it builds -- the user needs to be able to talk to it. Even at bootstrap, this is a basic script that connects the user to the LLM through the proxy.

**Success criteria:**
- [ ] `aba` command (with no stdin pipe) starts a conversational REPL
- [ ] First run loads BOOTSTRAP.md as system context
- [ ] User can type messages and get responses via the proxy (`http://127.0.0.1:8080/openai` or `/anthropic`)
- [ ] Conversation history maintained within the session (in-memory `Vec<Message>`)
- [ ] OAuth/API key setup guided by the agent itself on first run
- [ ] Ctrl-C or `exit` cleanly terminates

**What this looks like:** A simple read-eval-print loop, either as a bash/python bootstrap script or integrated into the Rust binary:
1. Load BOOTSTRAP.md as system prompt
2. Read user input from terminal
3. Send to LLM via proxy with conversation history
4. Print response
5. Repeat

**Not needed yet:** Persistence, search, threads, TUI, streaming.

**Architecture mapping:** This is the equivalent of Loom's `loom-cli` REPL. Loom's CLI connects to `loom-server` (LLM proxy); ABA's REPL connects to the nginx proxy. Same pattern, simpler implementation.

## Milestone 1: The Agent Loop Works

**Goal:** ABA can execute tools, run tests, and commit/revert autonomously within a single iteration.

Most of this exists today. This milestone is about hardening and making the state machine explicit.

**Success criteria:**
- [ ] Bash tool execution with stdout/stderr capture and exit code
- [ ] Multi-turn conversation: tool call -> result -> next LLM turn -> repeat
- [ ] PostToolsHook: configurable test command (not hardcoded `cargo test`) -> commit on pass, revert on fail
- [ ] Max turn limit (50) enforced to prevent runaway iterations
- [ ] Explicit state machine driving the loop, not ad-hoc conditionals

**State machine (from Loom's architecture):**
```
WaitingForUserInput -> CallingLlm -> ProcessingResponse -> ExecutingTools -> PostToolsHook -> CallingLlm (loop) or WaitingForUserInput
```

In headless mode (stdin pipe), `WaitingForUserInput` fires once. In conversational mode (Milestone 0), it fires after each PostToolsHook or when the LLM finishes without tool calls.

**What exists:** The agent loop, bash tool, PostToolsHook with git commit/revert. What's missing: explicit state machine, configurable test command, robust error handling for tool execution failures.

## Milestone 2: Ralph Loop

**Goal:** The outer loop feeds prompts to ABA repeatedly, surviving errors and making incremental progress.

**Success criteria:**
- [ ] `loop.sh` works reliably with plan and build modes
- [ ] `PROMPT_plan.md` generates `IMPLEMENTATION_PLAN.md` via gap analysis
- [ ] `PROMPT_build.md` picks and implements one task per iteration
- [ ] JJ commit/revert works alongside git (VCS trait with backend detection)
- [ ] Push to remote is optional and configurable (local-first by default)
- [ ] Loop survives agent errors: non-zero exit, LLM timeout, network failure
- [ ] Wall-clock timeout per iteration enforced by the loop script
- [ ] Iteration counter and basic progress reporting to stderr

**What exists:** `loop.sh` with plan/build modes, prompt files, GitVcs. What's missing: JJ backend, error resilience, iteration timeout, progress reporting.

## Milestone 3: The Agent Can Improve Itself

**Goal:** ABA successfully modifies its own Rust source code and passes its own test suite.

This is the defining milestone. Everything before this is infrastructure; this is where the Ralph Wiggum Loop philosophy pays off. ABA reads its specs, identifies gaps, implements changes, and the test suite gates every commit.

**Success criteria:**
- [ ] ABA reads `specs/` and `IMPLEMENTATION_PLAN.md`, identifies the highest-priority gap
- [ ] Implements a code change (new function, new tool, bug fix)
- [ ] `cargo test` and `cargo clippy` pass after the change
- [ ] Auto-commits the improvement with a meaningful commit message
- [ ] The improvement persists across iterations (committed to the repo)
- [ ] At least one concrete self-improvement demonstrated: ABA adds a tool, fixes a bug, or improves its own prompts

**Key risk:** The agent's context window may not be large enough to hold the full codebase plus specs plus conversation history. Mitigation: structured file reading tools (Milestone 1 Tier 1 tools) let the agent read only what it needs.

## Milestone 4: Observability and Health

**Goal:** Every LLM call, tool execution, and fitness check is logged, and costs are tracked.

See `specs/observability.md` for full requirements. This milestone implements the core subset needed to trust ABA running unattended.

**Success criteria:**
- [ ] Structured logging (JSON) for every LLM call, tool execution, and test result
- [ ] Token and cost tracking per iteration and cumulative per loop run
- [ ] Thread file per iteration: complete conversation transcript written to disk
- [ ] Loop health detection: stuck loops (same failure N times), revert spirals (no progress)
- [ ] Secret redaction in all persisted text (API key patterns scrubbed)
- [ ] Cost budget limits: optional max cost per iteration and per loop run
- [ ] Basic health summary to stderr at iteration end

**Not needed yet:** eBPF/kernel-level observability, web dashboards, `aba review` CLI.

## Milestone 5: Thread Persistence and Search

**Goal:** Conversations survive restarts and are searchable across sessions.

**Success criteria:**
- [ ] SQLite database for conversation storage (messages, metadata, loop references)
- [ ] FTS5 full-text search across all stored threads
- [ ] Resume previous conversations by thread ID
- [ ] Thread metadata: title, tags, workspace, creation time, message count
- [ ] Loop iterations link back to their parent thread for traceability
- [ ] Storage location configurable, defaults to `.aba/threads.db`
- [ ] Retention policy: configurable max age or count for old threads

## Milestone 6: Conversational CLI (Rust)

**Goal:** Replace any bootstrap REPL script with a proper Rust CLI that handles both conversational and headless modes.

See `specs/conversational-layer.md` for the full conversational layer design.

**Success criteria:**
- [ ] `aba` starts interactive REPL with streaming responses
- [ ] `aba loop [plan|build]` starts Ralph loop (replaces direct `loop.sh` invocation)
- [ ] `aba search "query"` searches thread history via FTS5
- [ ] `aba resume [thread-id]` resumes a previous conversation
- [ ] `aba threads` lists past conversations with summary info
- [ ] State machine drives the conversation flow (shared with agent loop)
- [ ] PostToolsHook runs after mutating tool calls in conversational mode
- [ ] LLM-generated commit messages (like Loom's auto-commit with Haiku)

## Milestone 7: VCS Evolution (JJ-Native)

**Goal:** Full Jujutsu integration as the primary VCS, with git as a fallback.

See `specs/source-control.md` for the full VCS architecture.

**Success criteria:**
- [ ] JJ as primary VCS backend (not git-colocated as a stepping stone)
- [ ] Operation log for full undo/audit trail of every agent action
- [ ] Workspace management: `jj workspace add` for parallel agent isolation
- [ ] Concurrent agent safety: no locking issues with multiple agents
- [ ] `Vcs::snapshot()` for explicit checkpointing before risky tool calls
- [ ] `Vcs::operation_log()` for the agent to inspect its own VCS history
- [ ] Backend auto-detection: `.jj/` -> JJ, `.git/` -> git, both -> prefer JJ

## Milestone 8: Multi-Agent / Weavers

**Goal:** ABA can spawn isolated execution environments for parallel work.

This is the equivalent of Loom's Kubernetes weaver pods, adapted for ABA's NixOS deployment model.

**Success criteria:**
- [ ] NixOS containers (or lightweight VMs) as "weavers" -- isolated execution environments
- [ ] Weaver lifecycle management: create, attach, destroy
- [ ] LLM calls from weavers route through the central nginx proxy
- [ ] Concurrent weavers with independent conversations and workspaces
- [ ] Resource limits per weaver (CPU, memory, disk)
- [ ] TTL-based cleanup: idle weavers destroyed after configurable timeout
- [ ] Shared `IMPLEMENTATION_PLAN.md` coordination across weavers
- [ ] JJ workspaces as the isolation primitive within a single repo

**Architecture mapping:** Loom uses Kubernetes pods with eBPF-based monitoring. ABA uses NixOS containers with systemd-based lifecycle management. The proxy pattern is identical: weavers talk to `localhost:8080`, the proxy injects auth.

## Milestone 9: System Health Loop

**Goal:** A dedicated Ralph loop that monitors, verifies, and auto-heals ABA itself.

This is a meta-loop: ABA watching ABA. It runs continuously alongside build loops and detects when ABA is broken, stuck, or degraded.

**Success criteria:**
- [ ] Health verification loop: separate from build loops, runs on a schedule
- [ ] Checks: `cargo test`, `cargo clippy`, proxy connectivity, disk space, weaver health
- [ ] Auto-remediation of detected issues (e.g., restart crashed weavers, clean stale locks)
- [ ] Deployment verification after self-updates: confirm new code boots and passes tests
- [ ] Crash tracking and session health metrics (uptime, error rates, restart counts)
- [ ] Alert mechanism: write to a known location, send webhook, or notify via proxy

## Milestone 10: Web UI

**Goal:** Browser-based interface for monitoring and interacting with ABA.

See `specs/conversational-layer.md` Tier 4 for the full web UI design.

**Success criteria:**
- [ ] Web dashboard showing active loops, thread history, and cost summaries
- [ ] Real-time streaming of loop output via SSE or WebSocket
- [ ] Thread browser with full-text search
- [ ] Visual diff viewer for loop iteration commits
- [ ] Approval UI for human-in-the-loop decisions (key requests, risky operations)
- [ ] Multi-project support: one ABA instance managing multiple repositories
- [ ] Served by the existing nginx proxy (new route, same infrastructure)

---

## Guiding Principles

These apply across all milestones:

1. **The REPL comes first.** The user must be able to talk to ABA before ABA can do anything. Milestone 0 is not optional infrastructure -- it is the entry point.

2. **One task per iteration.** Each Ralph loop picks one item from the plan, implements it, tests it, commits it. Small scope, atomic commits.

3. **Fresh context each time.** The loop restarts the agent, clearing conversation memory. The plan file is the shared state that persists.

4. **Tests as backpressure.** `cargo test` (or the configured test command) gates every commit. No tests pass, no commit lands. The agent learns through the revert penalty.

5. **The plan is shared state.** `IMPLEMENTATION_PLAN.md` persists between iterations and coordinates work. It is also disposable -- rerun plan mode to regenerate.

6. **Observe and tune.** Watch for failure patterns. Add guardrails to prompts, tighten specs, adjust the plan. The human is the manager; ABA is the worker.

7. **Local-first.** Everything works on localhost. Remote push is optional. The proxy runs on `127.0.0.1`. Weavers are local containers. The web UI is served locally. Production deployment is a layer on top, not a prerequisite.

8. **Security by architecture.** ABA never touches API keys. The proxy injects them. This is a hard boundary enforced by Linux user isolation, not a convention enforced by code.
