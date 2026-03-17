# Self-Bootstrapping Spec

## Overview

ABA's defining goal: it builds itself. The minimum viable agent loop is built by hand, then ABA Ralph-loops on its own codebase to implement the rest.

## What's Built by Hand (minimum to bootstrap)

1. LLM client abstraction with Anthropic + OpenAI backends
2. Multi-turn tool execution loop (call LLM → execute tools → feed results → repeat)
3. `bash` tool (the only tool needed to do everything else)
4. PostToolsHook: `cargo test` → commit on pass, revert on fail
5. Ralph loop infrastructure: `loop.sh`, `PROMPT_plan.md`, `PROMPT_build.md`
6. Specs describing what ABA should become (this file, agent-core.md, source-control.md)

## What ABA Builds Itself (via Ralph loop)

Prioritized by importance for improving the Ralph loop's effectiveness:

### Tier 1: Improve the agent's own capabilities
- [ ] `read_file` tool — structured file reading, better than bash cat
- [ ] `edit_file` tool — structured find/replace, less error-prone than sed
- [ ] `list_files` tool — directory listing with glob patterns
- [ ] `code_search` tool — ripgrep-powered search
- [ ] Tool registry pattern — dynamic tool registration, schema generation

### Tier 2: Improve the loop's effectiveness
- [ ] LLM-generated commit messages (like Loom's auto-commit with Haiku)
- [ ] Configurable test command (not hardcoded to `cargo test`)
- [ ] Structured output logging (JSON per tool call for audit trail)
- [ ] Thread file per iteration (audit trail reusable as context)
- [ ] Token usage tracking

### Tier 3: VCS evolution
- [ ] VCS trait wired through agent (replace direct git calls)
- [ ] JJ backend for VCS trait (`jj commit`, `jj undo`)
- [ ] Workspace detection (`.jj/` vs `.git/`)

### Tier 4: Conversational layer
- [ ] Interactive CLI mode — `aba` with no stdin starts conversational REPL
- [ ] Thread persistence (SQLite + FTS5), conversation history and search
- [ ] Multi-loop management — spawn, monitor, steer concurrent Ralph loops
- [ ] Web UI for loop monitoring, thread browsing, cost dashboards
- See `specs/conversational-layer.md` for full spec

### Tier 5: Multi-agent support
- [ ] Workspace management (JJ workspaces or git worktrees)
- [ ] Parallel agent spawning
- [ ] Shared IMPLEMENTATION_PLAN.md coordination

### Tier 6: Observability & OODA loop modes
Reference: [jomadu/ralph-wiggum-ooda](https://github.com/jomadu/ralph-wiggum-ooda) — decomposes the Ralph loop into OODA phases (Observe, Orient, Decide, Act) with separate prompts per phase. Currently ABA only has plan/build modes (Huntley's canonical "3 Phases, 2 Prompts, 1 Loop"). As ABA matures, observe/orient could become automated loop modes rather than purely human activities — an agent that reviews iteration history, identifies failure patterns, and tunes prompts or specs.
- [ ] Structured telemetry per tool call (name, args, result, duration, tokens)
- [ ] Iteration summary file (what was attempted, what succeeded/failed)
- [ ] Dashboard or log viewer for monitoring Ralph loops

## Bootstrap Flow

On first run in a new project, ABA looks for a `BOOTSTRAP.md` file for initial orientation — project structure, language, test commands, conventions. This is a one-shot setup flow that primes ABA before any Ralph loops run. No conversational layer is needed for bootstrap; it feeds directly into the agent as context.

## How to Run the Bootstrap

```bash
# 1. First, generate the implementation plan
./loop.sh plan

# 2. Review IMPLEMENTATION_PLAN.md, adjust if needed

# 3. Run the build loop
./loop.sh

# 4. Watch, observe, tune prompts as needed (Ctrl+C to stop)
```

## Key Principles (from the Ralph Playbook)

- **One task per iteration** — each loop picks one item from IMPLEMENTATION_PLAN.md
- **Fresh context each time** — the bash loop restarts the agent, clearing context
- **Backpressure via tests** — `cargo test` + `cargo clippy` gate every commit
- **IMPLEMENTATION_PLAN.md is shared state** — persists on disk between iterations
- **The plan is disposable** — if it's wrong, rerun `./loop.sh plan` to regenerate
- **Observe and tune** — watch for failure patterns, add guardrails to prompts or AGENTS.md
