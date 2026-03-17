# CLI Interfaces — Loom Reference

Source: `docs/research/loom-cli-tui-web-acp.md` (ABA agent research session, 2026-03-17)
References: [ghuntley/loom](https://github.com/ghuntley/loom) · [AGENTS.md](https://github.com/ghuntley/loom/blob/trunk/AGENTS.md) · [specs/](https://github.com/ghuntley/loom/tree/trunk/specs) · [agentclientprotocol.com](https://agentclientprotocol.com/) · [ghuntley.com/loop](https://ghuntley.com/loop/)

---

## 1. loom-cli — Command Reference

Loom-cli is the primary user-facing binary. It communicates with `loom-server` over HTTP (`/proxy/{provider}` and `/stream` endpoints), with SSE streaming for responses. API keys are held server-side; the CLI never contacts LLM providers directly.

| Command | Purpose | Key flags / notes |
|---|---|---|
| `loom login` | Authenticate against the loom server | Supports GitHub, Google, Okta, device code, magic links |
| `loom version` | Print binary version | — |
| `loom update` | Self-update the binary | Fetches from `/bin/{platform}` endpoint; also a NixOS auto-update workflow |
| `loom resume` | Resume a previous session | Restores from SQLite thread store |
| `loom search` | Full-text search across conversation threads | Backed by SQLite FTS |
| `loom list` | List active / past threads | — |
| `loom private` | Mark a thread private | — |
| `loom new` | Create a new weaver (remote agent pod) | `--image <image>` |
| `loom attach <weaver-id>` | Attach REPL to a running weaver | `--server-url` flag for server address |
| `loom weaver ps` | List running weavers | — |
| `loom weaver delete <weaver-id>` | Terminate a weaver pod | — |
| `loom acp-agent` | Start the agent in ACP (editor) mode | JSON-RPC 2.0 over stdio |

Full invocation pattern uses `--server-url`:

```
loom --server-url https://loom.ghuntley.com <subcommand>
```

---

## 2. REPL UX Flow

The interactive REPL follows a deterministic loop:

```
1. User types prompt (or prompt arrives via stdin / ACP)
2. CLI sends request to loom-server → proxied to LLM provider via SSE stream
3. Response tokens stream back in real time; partial output renders in terminal
4. LLM emits tool_call blocks → CLI displays pending tool panel
5. [Interactive mode] confirmation modal shown; user approves/rejects
   [Autonomous mode]  --dangerously-skip-permissions bypasses approval
6. Tool executes; result appended to conversation as tool_result
7. Result sent back to LLM; loop continues (max 50 tool turns per iteration)
8. LLM emits stop → PostToolsHook fires:
     a. Fitness check runs (e.g. cargo test)
     b. Pass → git commit (auto-commit)
     c. Fail → git reset --hard (revert)
9. Iteration complete; REPL prompt returns
```

Each outer Ralph loop iteration (driven by `loop.sh`) is stateless — a fresh process that reads the on-disk state, git history, and prompt file.

---

## 3. Interactive vs Autonomous Modes

| Mode | Trigger | Behaviour |
|---|---|---|
| Interactive | Default | TUI renders; tool calls show confirmation modal; user approves before execution |
| Autonomous | `--dangerously-skip-permissions` flag | All tool calls auto-approved; no human gate; agents push directly to master |

In Loom's production usage the agents run with full sudo on a NixOS host. Deployments complete within 30 seconds. Code review is omitted — the ralph loop is the review.

---

## 4. loom-tui — Ratatui Widget Inventory

Built with Ratatui 0.30; uses a component system with visual snapshot testing.

| Crate / component | Widget | Purpose |
|---|---|---|
| `loom-tui-message-list` | Scrollable list | Renders conversation turns; each turn styled by role |
| `loom-tui-input` | Text input box | Multi-line prompt entry; supports paste |
| `loom-tui-tool-panel` | Panel overlay | Shows pending/running/completed tool calls with status icons |
| `loom-tui-thread-sidebar` | Side panel | Thread list with search and filter |
| `loom-tui-status-bar` | Status line | Token counts, model name, connection state |
| `loom-tui-modal` | Modal dialog | Confirmation prompts for tool approval (interactive mode) |
| `loom-tui-spinner` | Throbber widget | Indeterminate activity indicator during LLM streaming (`throbber-widgets-tui`) |
| `loom-tui-progress` | Gauge / LineGauge | Determinate progress (e.g. file uploads, test runs) |
| `loom-tui-diff` | Diff block | Inline diff display for file edits proposed by the agent |
| `loom-tui-markdown` | Paragraph renderer | Renders markdown-formatted LLM output (bold, code fences, lists) |

Third-party Ratatui crates used: `throbber-widgets-tui` (spinners), `ratatui-widgets` (extended widget collection).

---

## 5. loom-web — Svelte 5 Frontend

Stack: **SvelteKit + Tailwind CSS**. Mandatory rule in AGENTS.md: always use **Svelte 5 runes syntax**; never use Svelte 4 patterns.

| Feature | Detail |
|---|---|
| Conversation UI | Web-based chat interface; SSE streaming renders tokens progressively |
| Code display | Syntax-highlighted code blocks |
| Diff viewer | Side-by-side or unified diff for proposed file changes |
| Agent control panel | Start / stop / attach to weavers; view iteration status |
| Session management | Browse threads, resume, mark private |
| OAuth flow | Handles redirect-based auth (GitHub, Google, Okta) in-browser |

Location in repo: `web/loom-web/`. Uses the same `loom-server` HTTP backend as the CLI.

---

## 6. Agent Client Protocol (ACP)

ACP is an open standard (by Zed Industries) for connecting code editors to coding agents. It is analogous to LSP for language servers.

**Transport:** JSON-RPC 2.0 over stdio (stdin/stdout pipes). The editor spawns the agent as a subprocess and communicates through piped streams.

| Aspect | Detail |
|---|---|
| Protocol | JSON-RPC 2.0 over stdio |
| Loom command | `loom acp-agent` |
| Supported editors | Zed, Neovim, VSCode (IntelliJ / JetBrains: coming soon) |
| Reference impl | Gemini CLI (by Google) |
| Other ACP agents | Claude Agent, Codex, GitHub Copilot, OpenCode, Kiro |
| Key benefit | No vendor lock-in; editor and agent communicate through one standardised protocol |

ACP in loom's specs covers: VS Code extension via ACP, Git context tracking in threads, auto-commit after tool execution.

Note: as of the research date (2026-03-17) there was no confirmation from search results that ghuntley/loom has shipped a production ACP implementation; it appears in the specs directory as a planned interface.

---

## 7. conversation_recall Tool

`conversation_recall` is a built-in agent tool (not a CLI command) that allows the LLM to query its own conversation history when the context window fills up.

| Property | Detail |
|---|---|
| Type | Agent tool (available inside the tool loop) |
| Backing store | SQLite with full-text search (FTS) |
| Capabilities | Retrieve specific turns, tool call history, full-text search across all sessions |
| Trigger | Called automatically by the LLM when it needs older context |
| Persistence | Every conversation turn is stored verbatim; turns drop out of the context window but remain searchable |
| Session resume | `loom resume` reloads a thread from SQLite to continue a previous session |

This is distinct from the web `loom search` command (which the human uses) — `conversation_recall` is the LLM's self-service recall mechanism.

---

## 8. Tool System Overview

Tools are defined with a standard JSON schema: `{ name, description, parameters }`.

Categories (11+):

| Category | Examples |
|---|---|
| Files | read, write, edit, delete, move (with fuzzy matching) |
| Shell | bash execution with git safety guards |
| Search | ripgrep, glob patterns |
| Web | fetch URL, web search |
| Code analysis | tree-sitter (when installed), regex fallback |
| Math | calculator, spreadsheet operations |
| Documents | generation and formatting |
| Task tracking | task list management |
| Conversation | `conversation_recall` |
| Git | commit, reset, status, diff |
| Utilities | miscellaneous OS operations |

Max 50 tool turns per agent iteration. Tool execution is callback-based; different LLM providers format tool_call blocks differently but all funnel through the same execution callback.

---

## 9. Self-Update Mechanism

| Step | Detail |
|---|---|
| Command | `loom update` |
| Distribution endpoint | `/bin/{platform}` on the loom server |
| NixOS integration | `.agents/workflows/watch-nixos-auto-update.md` — agent workflow that watches for NixOS updates and applies them |
| Nix flake | Reproducible builds; flake provides pinned toolchain |

The self-update is part of the ralph loop philosophy: the system updates itself autonomously the same way it builds software.

---

## 10. Key Insights for ABA

**NEW — ACP is the emerging standard for editor integration.** ABA currently has no editor integration. Implementing `loom acp-agent`-style behaviour (JSON-RPC 2.0 over stdio) would make ABA usable inside Zed, Neovim, and JetBrains without editor-specific plugins. This is a direct build target implied by `specs/self-bootstrapping.md`.

**NEW — `conversation_recall` solves the context-window problem.** ABA has no equivalent. Adding a SQLite-backed conversation store with an LLM-callable `conversation_recall` tool would allow indefinitely long agent sessions. The sfw/loom phrasing is worth copying verbatim into a spec: "persists every turn verbatim; model has a tool to retrieve anything it needs."

**NEW — `--dangerously-skip-permissions` is the autonomous-mode switch.** ABA's `loop.sh` currently passes this implicitly by running non-interactively. Making it an explicit named flag in ABA's CLI would align nomenclature with the emerging ecosystem norm and make the distinction interactive/autonomous clear to operators.

**CONFIRMED — SSE streaming over HTTP proxy is Loom's chosen architecture.** ABA currently calls LLM APIs directly from the binary. Introducing a local `loom-server`-equivalent proxy would: (a) keep API keys server-side, (b) enable the web frontend to share the same stream, (c) allow the TUI and CLI to be thin clients. This is a larger refactor but is the architectural direction Loom has validated.

**CONFIRMED — 50-turn tool limit per iteration.** ABA already implements this (`agent.rs`, max 50 tool turns). This is the correct default; Loom independently converged on the same number.

**CONFIRMED — Ratatui is the correct TUI stack for ABA.** The loom-tui-\* crates confirm Ratatui 0.30 is viable for all required widgets. If ABA adds a TUI, the widget inventory in section 4 is the shopping list.

**NEW — Weavers = ephemeral K8s pods.** Loom's remote execution model uses Kubernetes pods (`loom-weavers` namespace, full sudo, NixOS, deploy < 30s). This is not in ABA's current scope but is how Loom scales to multiple simultaneous ralph loops.

---

*Sources: [ghuntley/loom](https://github.com/ghuntley/loom) · [AGENTS.md](https://github.com/ghuntley/loom/blob/trunk/AGENTS.md) · [agentclientprotocol.com](https://agentclientprotocol.com/) · [ratatui.rs](https://ratatui.rs/) · [ghuntley.com/loop](https://ghuntley.com/loop/)*
