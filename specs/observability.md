# Observability & Monitoring

## Overview

A headless agent loop running unattended is only as useful as its audit trail. If you can't see what the agent did, why it did it, and what it cost, you can't tune the prompts or trust the output. ABA must treat observability as a first-class concern — every LLM call, tool execution, and fitness check must be captured, stored, and reviewable.

Inspired by Loom's approach: the agent's conversation logs are the primary debugging surface. Git history is the secondary one.

## Requirements

### 1. Everything Is Logged

Every interaction boundary must produce a structured log entry:

- **LLM requests** — model, system prompt hash, message count, tool definitions sent, token counts (prompt + completion)
- **LLM responses** — text content (truncated in logs), tool calls returned, stop reason, latency
- **Tool executions** — tool name, arguments, stdout/stderr (truncated), exit code, wall-clock duration
- **PostToolsHook** — test command, pass/fail, commit hash or revert confirmation

Log entries must be structured (JSON or equivalent) so they can be parsed programmatically. Human-readable stderr output via `tracing` is complementary, not a replacement.

### 2. Thread / Audit Trail

Each Ralph loop iteration produces a **thread file**: the complete conversation transcript for that iteration.

- One file per iteration, named by timestamp and iteration number (e.g., `threads/2024-01-15T10-30-00_iter_007.json`)
- Contains the full `Vec<Message>` history: system prompt, user prompt, all assistant responses, all tool calls and results
- Written atomically at the end of each iteration (success or failure)
- Thread files are the primary artifact for post-hoc review ("what did the agent do on iteration 7?")

### 3. Conversation Log Management

Thread files accumulate. They must be manageable:

- **Storage location** — configurable directory, defaults to `.aba/threads/` in the repo root
- **Rotation** — old threads can be archived or deleted by age or count (configurable retention policy)
- **Searchability** — thread files are JSON; standard tools (`jq`, `grep`) work. A future `aba log` subcommand could provide filtered views (e.g., "show all iterations that reverted", "show tool calls that failed")
- **Size limits** — individual tool outputs within threads should be truncated at a configurable max (default 10KB per tool result) to prevent multi-GB log files from long bash outputs

### 4. Cost Tracking

Token usage and API costs must be tracked per iteration and cumulatively:

- **Per LLM call** — prompt tokens, completion tokens, model name
- **Per iteration** — sum of all LLM calls in that multi-turn conversation
- **Per loop run** — cumulative across all iterations in a `loop.sh` invocation
- **Cost estimation** — map token counts to approximate dollar costs using known model pricing (configurable rate table)
- **Budget limits** — optional max cost per iteration and per loop run; exceed → abort with a clear message

Cost data is recorded in the thread file and optionally emitted as a summary line to stderr at iteration end.

### 5. Backpressure Metrics

Track the fitness signals that indicate whether the loop is making progress:

- **Test pass/fail rate** — rolling window over recent iterations
- **Commit/revert ratio** — how often does the agent's work survive the fitness check?
- **Iteration count** — total iterations, iterations since last successful commit
- **Tool call volume** — average tool calls per iteration (rising counts may indicate flailing)

These metrics inform tuning decisions. They should be available in the thread files and optionally summarized to stderr.

### 6. Loop Health Detection

The outer loop (or the agent itself) should detect pathological states:

- **Stuck loop** — the same test failure repeating across N consecutive iterations (configurable threshold, default 3). Action: log a warning, optionally halt
- **Runaway iteration** — an iteration exceeding MAX_TOOL_TURNS is already handled by the agent; additionally, wall-clock timeout per iteration should be enforced by `loop.sh`
- **Context exhaustion** — if the conversation history approaches the model's context window, the agent should detect this (via token count tracking) and gracefully terminate the iteration rather than hitting a truncation error
- **Revert spiral** — if the last N iterations all reverted (no forward progress), the loop should pause or alert rather than burning tokens indefinitely

### 7. Secret Redaction

Conversation logs must never contain secrets. This is critical for the proxy pattern where API keys should never appear in any stored artifact.

- **Environment variable scrubbing** — before writing thread files, scan for patterns matching known secret formats (API keys, tokens, passwords) and replace with `[REDACTED]`
- **Known key patterns** — at minimum: `sk-*`, `Bearer *`, anything matching `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OAUTH_CLIENT_ID` values from the environment
- **Tool output scrubbing** — bash tool results that echo environment variables or config files must be scrubbed before logging
- **No secrets in LLM context** — the system prompt and tool definitions should never include raw API keys; auth is handled by the client, not passed through the conversation

### 8. Human Review Surface

An engineer supervising a Ralph loop needs two things to understand what happened:

1. **Git log** — `git log --oneline` shows which iterations produced commits and what they changed. This is already free by virtue of auto-commit.
2. **Conversation transcripts** — the thread files from requirement 2. Together with git log, an engineer can correlate "commit X was produced by iteration Y" and read the full conversation that led to it.

A future `aba review` subcommand could present a unified view: iteration number, pass/fail, commit hash (if any), cost, and a link to the thread file. For now, the raw files plus git log are sufficient.

### 9. Kernel-Level Observability (Future)

Loom uses eBPF to attach observability probes to weaver (agent) processes at the kernel level — tracking syscalls, file I/O, network activity, and resource consumption without modifying the agent code.

This is a future consideration for ABA, relevant when running multiple agents in production:

- **Process-level metrics** — CPU, memory, file descriptors per agent process
- **Syscall tracing** — what files the agent reads/writes, what network calls it makes
- **Container-aware** — if agents run in containers (Docker/Nix), eBPF probes can monitor from the host

This is out of scope for the initial implementation but noted here as the direction for production-grade multi-agent observability.

## Implementation Priority

Aligned with the tiers in `self-bootstrapping.md`:

1. **Structured JSON logging per tool call** — extend existing `tracing` output (Tier 2)
2. **Thread file per iteration** — write conversation transcript to disk (Tier 2)
3. **Token/cost tracking** — parse usage from LLM responses, accumulate (Tier 2)
4. **Secret redaction** — scrub thread files before writing (Tier 2, security-critical)
5. **Backpressure metrics and loop health** — add to `loop.sh` and agent (Tier 5)
6. **Log management and review CLI** — `aba log`, `aba review` (Tier 5)
7. **eBPF / kernel-level probes** — future, multi-agent production (beyond current tiers)
