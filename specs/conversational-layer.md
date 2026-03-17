# Conversational Layer

## Overview

The conversational layer is the interface between humans and Ralph loops. A human says "I want to achieve X" and ABA creates and manages the appropriate loop(s) to get there. This is inspired by Geoffrey Huntley's Loom architecture (loom-cli + loom-server + loom-web), but ABA builds toward it incrementally — using its own Ralph loops.

## Two Layers

ABA has two distinct execution layers:

```
Human ↔ [Conversational Layer] ↔ [Loop Layer] ↔ LLM + Tools
```

**Conversational Layer** — Human-facing dialogue with memory. Understands intent, decomposes goals into tasks, spawns and monitors Ralph loops, reports results. Stateful across sessions (thread persistence).

**Loop Layer** — Autonomous Ralph execution. Single-objective, stateless per iteration, gated by fitness checks. This is what exists today (`loop.sh` + PROMPT files + agent core).

The conversational layer is a manager; the loop layer is a worker. The human manages the conversational layer; the conversational layer manages the loops.

## Thread Persistence

Conversations are stored and searchable, modeled after Loom's FTS5 thread storage:

- Each conversation is a **thread** with a unique ID
- Threads contain messages (human + assistant), loop references, and metadata
- SQLite with FTS5 for full-text search across thread history
- Threads are resumable — pick up where you left off
- Loop iterations link back to their parent thread for traceability

## Session Management

The conversational layer manages multiple concurrent loops:

- **Spawn**: Start a new Ralph loop for a given objective
- **Status**: Overview of all active loops (running, succeeded, failed, iteration count)
- **Steer**: Adjust a running loop's prompt or objective mid-flight
- **Stop**: Gracefully halt a loop (finish current iteration, don't start next)
- **Review**: Inspect a loop's iteration history, diffs, test results

This replaces the current model of one terminal per loop, SSH-managed.

## Bootstrap Mode

On first run in a new project, ABA loads `BOOTSTRAP.md` (if present) for initial orientation:

- Project structure, language, test commands, conventions
- What ABA should know before starting any loops
- No conversational layer needed — bootstrap is a one-shot setup flow
- After bootstrap, ABA is ready for either conversational mode or direct Ralph loops

This is analogous to Loom's "just read the README" philosophy but explicit and structured.

## The Proxy Pattern

All LLM calls route through a server-side proxy, regardless of UI surface:

```
CLI ──┐
TUI ──┤── Proxy (auth, rate limits, cost tracking, logging) ──→ LLM API
Web ──┘
```

The proxy handles auth, rate limiting, cost tracking, and structured logging. UI clients are thin — they render conversations and relay user input. This is already implemented in the current architecture and remains the pattern as new surfaces are added.

## Progressive Build

Each UI surface is built by ABA itself via Ralph loops, in order of complexity:

1. **CLI** (stdin/stdout) — already exists as the loop interface
2. **Interactive CLI** — `aba` with no stdin enters conversational mode (REPL)
3. **TUI** — terminal UI with panes for conversation, loop status, logs
4. **Web** — browser-based UI (like loom-web), served by the proxy

Each layer builds on the previous. The CLI conversational mode is the foundation; TUI and web are rendering surfaces on top of the same conversational engine.

## Implementation Tiers

### Tier 0: Current State

What exists today. Human manages loops manually.

- PROMPT files define loop objectives
- `loop.sh` runs the outer loop
- Human monitors via terminal, steers by editing prompts
- No conversation persistence, no session management

### Tier 1: Interactive CLI Mode

`aba` with no stdin starts a conversational REPL.

- Human types objectives in natural language
- ABA decomposes into tasks, spawns Ralph loops
- Loop output streams to the terminal
- Conversation history kept in memory (session-scoped, not persisted)
- `aba run <prompt-file>` remains for headless/scripted use

### Tier 2: Thread Persistence

Conversations survive across sessions.

- SQLite database for thread storage (messages, metadata, loop references)
- FTS5 full-text search across all threads
- `aba threads` — list past conversations
- `aba resume <thread-id>` — continue a previous conversation
- Loop iteration results stored as structured records

### Tier 3: Multi-Loop Management

Concurrent weavers with a unified control plane.

- Spawn multiple Ralph loops from a single conversation
- Status dashboard (which loops are running, iteration counts, pass/fail rates)
- Loop coordination — shared plans, dependency ordering
- Background execution with notification on completion or failure
- JJ workspaces or git worktrees for parallel loop isolation

### Tier 4: Web UI

Browser-based interface, served by the proxy.

- Real-time loop monitoring (WebSocket stream of iteration events)
- Thread browser with search
- Visual diff viewer for loop iterations
- Cost and token usage dashboards
- Multi-project support
