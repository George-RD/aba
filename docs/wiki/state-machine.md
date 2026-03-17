# Loom Agent State Machine

Reference for Loom's `loom-core` state machine architecture.
Compressed from `docs/research/loom-core-state-machine.md`, `docs/research/loom-error-handling-recovery.md`, and `docs/research/loom-vcs-spool-autocommit.md`.

Sources: [ghuntley/loom](https://github.com/ghuntley/loom), [specs/state-machine.md](https://github.com/ghuntley/loom/blob/trunk/specs/state-machine.md), [verification.md](https://github.com/ghuntley/loom/blob/trunk/verification.md).

---

## Design Principle: Pure State Machine

```
fn handle_event(state: AgentState, event: AgentEvent) -> AgentAction
```

- **No I/O inside the state machine.** `handle_event` takes an event, returns an action; the caller executes the action and feeds the result back as the next event.
- Deterministic: same state + same event always produces same action.
- Testable in isolation — no async, no network, no filesystem dependencies.

---

## States

| State | Description |
|---|---|
| `WaitingForUserInput` | Idle. Awaiting user or external trigger. |
| `CallingLlm` | LLM request in flight. |
| `ProcessingLlmResponse` | Parsing and validating the LLM response. |
| `ExecutingTools` | Running tool calls requested by the LLM. |
| `PostToolsHook` | Post-execution hook: auto-commit, logging, cleanup. |
| `Error` | Handling a failure. Manages retry countdown. |
| `ShuttingDown` | Terminal state. Cleanup then exit. |

---

## Events

| Event | Trigger |
|---|---|
| `UserInputReceived` | User submits input (REPL or stdin) |
| `LlmResponseReceived` | HTTP response from LLM provider arrives |
| `ToolsRequested` | LLM response contains tool_use blocks |
| `ToolExecutionCompleted` | All requested tools have finished |
| `PostToolsHookCompleted` | Hook finished (commit succeeded or skipped) |
| `ErrorOccurred` | LLM call, tool execution, or parse failure |
| `RetryTimeoutFired` | External timer fires after backoff delay |
| `ShutdownRequested` | User Ctrl-C or max retries exhausted |

---

## Actions

| Action | What the caller does |
|---|---|
| `SendLlmRequest` | Serialize conversation, call LLM API, await response |
| `ExecuteTools` | Run each tool call, collect results |
| `DisplayMessage` | Print text to user |
| `RunPostToolsHook` | Trigger auto-commit + logging subsystem |
| `RetryWithBackoff(delay)` | Set a timer for `delay`, fire `RetryTimeoutFired` when done |
| `WaitForInput` | Block on stdin / REPL |
| `Continue` | Internal: no user-visible action, advance immediately |
| `Shutdown` | Exit process cleanly |

---

## Full Transition Table

| Current State | Event | Action | Next State |
|---|---|---|---|
| `WaitingForUserInput` | `UserInputReceived` | `SendLlmRequest` | `CallingLlm` |
| `WaitingForUserInput` | `ShutdownRequested` | `Shutdown` | `ShuttingDown` |
| `CallingLlm` | `LlmResponseReceived` (text only) | `DisplayMessage` | `ProcessingLlmResponse` |
| `CallingLlm` | `LlmResponseReceived` (has tool_use) | `Continue` | `ProcessingLlmResponse` |
| `CallingLlm` | `ErrorOccurred` | `RetryWithBackoff(delay)` | `Error` |
| `CallingLlm` | `ShutdownRequested` | `Shutdown` | `ShuttingDown` |
| `ProcessingLlmResponse` | `ToolsRequested` | `ExecuteTools` | `ExecutingTools` |
| `ProcessingLlmResponse` | (no tools) | `WaitForInput` | `WaitingForUserInput` |
| `ProcessingLlmResponse` | `ErrorOccurred` | `RetryWithBackoff(delay)` | `Error` |
| `ExecutingTools` | `ToolExecutionCompleted` | `RunPostToolsHook` | `PostToolsHook` |
| `ExecutingTools` | `ErrorOccurred` | `RetryWithBackoff(delay)` | `Error` |
| `PostToolsHook` | `PostToolsHookCompleted` | `SendLlmRequest` | `CallingLlm` |
| `PostToolsHook` | `ErrorOccurred` | `RetryWithBackoff(delay)` | `Error` |
| `Error` (retries < max) | `RetryTimeoutFired` | `SendLlmRequest` | `CallingLlm` |
| `Error` (retries >= max) | `RetryTimeoutFired` | `DisplayMessage` + `WaitForInput` | `WaitingForUserInput` |
| `Error` | `ShutdownRequested` | `Shutdown` | `ShuttingDown` |
| `ShuttingDown` | (any) | `Shutdown` | `ShuttingDown` |

---

## Retry / Backoff Logic

Retry state tracks `ErrorOrigin` to apply different strategies per failure type.

### LLM Failures (`ErrorOrigin::Llm`)

| Parameter | Anthropic | OpenAI |
|---|---|---|
| Max attempts | 3 | 3 |
| Base delay | 200ms | 500ms |
| Backoff multiplier | 2× | 2× |
| Max delay | 5s | 30s |
| Jitter | Yes (prevents thundering herd) | Yes |

Formula: `delay = min(base_delay × (2 ^ attempt) + jitter, max_delay)`

### Error Classification

| Category | HTTP Codes | Retryable |
|---|---|---|
| Transient | 408, 429, 500, 502, 503, 504 + network timeouts | Yes |
| Permanent | 400, 401, 403, 404, 422 | No — goes to `WaitingForUserInput` immediately |

### Tool Failures (`ErrorOrigin::Tool`)

- Separate retry logic (shorter, no jitter documented)
- Repeated tool failures transition to `ShuttingDown` or `WaitingForUserInput` with error

---

## PostToolsHook: Auto-Commit

The `PostToolsHook` state runs after every `ExecutingTools` round. It uses `loom-auto-commit` and `loom-git`.

**Trigger condition:** Only fires when a mutating tool was called (`bash`, `edit_file`). Read-only tools skip it.

**Sequence:**
1. Check `git diff` for staged/unstaged changes
2. If changes exist: call Claude Haiku to generate a commit message
3. Stage all changes and commit
4. Return `PostToolsHookCompleted`; loop continues to `CallingLlm`

**Commit message format:** Conventional commits — `<type>(<scope>): <description>` (< 72 chars)
- Type inferred: `feat` (new file), `fix` (modified), `test` (test file), `chore` (build/config)
- Scope from file path

**Diff size limit:** 32KB truncation with a truncation notice embedded in the commit message.

**Fail-open:** Auto-commit failure does NOT block the agent. `PostToolsHookCompleted` fires regardless — the hook never kills the loop.

**Fallback message:** `"chore: auto-commit from loom [LLM unavailable]"`

**Configurable:** `LOOM_AUTO_COMMIT_DISABLE=1` disables the hook entirely.

---

## Conversation Model

```
Message {
    role: user | assistant | tool,
    content: Vec<ContentBlock>,
}

ContentBlock =
    | Text(String)
    | ToolUse { id, name, input: serde_json::Value }
    | ToolResult { tool_use_id, content: String, is_error: bool }
```

- Full conversation history sent on every LLM call (no summarization)
- Tool results are injected as `ToolResult` blocks in the `user` role turn after execution
- Thread persistence handled by `loom-thread` (separate from state machine)

---

## AgentConfig

Configured via TOML (`~/.config/loom/config.toml`) + environment variables. Environment variables take precedence.

| Field | Env Var | Description |
|---|---|---|
| `model` | `LOOM_LLM_MODEL` | LLM model identifier |
| `provider` | `LOOM_LLM_PROVIDER` | `anthropic` or `openai` |
| `temperature` | — | Sampling temperature |
| `max_tokens` | — | Response length cap |
| `tools` | — | Enabled tool list |
| `max_retries` | — | Max retries before giving up |
| `retry_base_delay_ms` | — | Backoff base delay |
| `auto_commit` | `LOOM_AUTO_COMMIT_DISABLE` | Enable/disable PostToolsHook |

Note: The research found an env var rename — `LOOM_PROVIDER` was renamed to `LOOM_LLM_PROVIDER`. ABA should use the correct name.

---

## Testing

| Test Type | Coverage |
|---|---|
| Unit tests | Per-state `handle_event` transitions (pure, no I/O) |
| Property-based tests | 8 branch protection property tests (proptest crate) |
| Integration tests | 193 authorization tests (org, thread, repo, team, user, session) |

Property-based tests use `proptest` with shrinking — counterexamples are minimized before reporting.

The pure `handle_event` design makes state machine tests trivially fast: no mocks needed.

---

## ABA vs. Loom: State Machine Gap Analysis

| Feature | Loom | ABA (current) |
|---|---|---|
| Explicit state enum | `AgentState` enum with 7 variants | Implicit (loop structure) |
| Pure `handle_event` | Yes | No — I/O mixed into loop |
| Error state with retry | Yes — `ErrorOrigin`, exponential backoff | No — errors propagate/panic |
| PostToolsHook state | Yes — dedicated state, auto-commit | Partial — `PostToolsHook` runs `cargo test` + commit/revert |
| `ShutdownRequested` event | Yes | No |
| Error classification (transient/permanent) | Yes | No |
| Property-based tests | Yes | No |

---

## Key Insights for ABA

**1. Extract a pure state machine.** ABA's `agent.rs` loop mixes I/O and state logic. Refactoring to a `handle_event(state, event) -> action` pattern would make every state transition unit-testable without network calls.

**2. Add `ErrorOrigin` to distinguish failure types.** A single error path means LLM 429s get treated the same as tool panics. Separate paths let you apply rate-limit backoff only where it helps.

**3. `ShuttingDown` is a real state, not just `process::exit`.** Loom ensures cleanup (flush logs, close DB connections) before exit. ABA currently hard-exits on Ctrl-C.

**4. PostToolsHook should be fail-open.** If `cargo test` is the fitness gate, its failure should trigger `git reset` and re-loop (which ABA does), but commit failure should not kill the agent.

**5. Jitter on retry is important at scale.** Even a single agent hitting a rate-limited provider benefits from jitter — if the outer `loop.sh` restarts quickly, without jitter you get synchronized hammering.

**6. Context window is managed at the loop level, not the state machine level.** Loom's answer to context exhaustion is fresh context per iteration (via the Ralph outer loop), not in-flight truncation. ABA's `loop.sh` already follows this — keep it.

**7. Property-based tests for state transitions.** Because `handle_event` is pure, you can fuzz it with random event sequences to find unreachable or panicking states. This is high-leverage for a small test investment.
