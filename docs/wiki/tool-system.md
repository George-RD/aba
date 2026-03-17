# Tool System

Reference for Loom's tool system and ABA's current implementation.
Compressed from `docs/research/loom-tool-system.md`.

---

## Loom Tool Catalog

Loom defines 7 verified tools in the `loom-tools` crate.

### Tool Table

| Tool | Mutating | PostToolsHook | Timeout | Engine |
|------|----------|---------------|---------|--------|
| `bash` | Yes | Yes | default 60s, max 300s | `std::process::Command` |
| `edit_file` | Yes | Yes | not specified | string-match diff |
| `read_file` | No | No | not specified | filesystem |
| `list_files` | No | No | not specified | filesystem |
| `oracle` | No | No | not specified | LLM API call |
| `web_search` | No | No | not specified | external search API |
| `code_search` | No | No | not specified | ripgrep |

### bash

- **Input**: `command` (required, string), `cwd` (optional, string), `timeout_secs` (optional, int; default 60, max 300)
- **Output**: stdout + stderr captured; exit code returned
- **Notes**: Executes with environment preserved across tool calls in the same turn. stdout/stderr both returned.

### edit_file

- **Input**: `path` (required, string), `edits[]` (required, array of `{old_str, new_str}`), `replace_all` (optional, bool)
- **Output**: confirmation of success or failure
- **Strategy**: Exact string matching — not regex. Requires precise match of existing content. Uses unified diff internally. Single edit per call recommended; `replace_all` replaces every occurrence.

### read_file

- **Input**: `path` (required, string)
- **Output**: file contents as string

### list_files

- **Input**: `path` (required, string)
- **Output**: directory listing

### oracle (LLM Delegation — Special Attention)

- **Input**: `query` (required, string), `model` (optional, string), `max_tokens` (optional, int), `temperature` (optional, float 0–1), `system_prompt` (optional, string)
- **Output**: LLM response text
- **Purpose**: One LLM calls another LLM as a tool. Enables agent-to-agent delegation without spawning a subprocess — the parent agent issues a direct API call to a second model (e.g., Claude calling GPT, or Claude calling a specialized fine-tune).
- **Classification**: Read-only from the local filesystem perspective, but it consumes tokens and latency. No PostToolsHook required.
- **Use cases**: Research, planning validation, cross-model verification, routing specialized tasks to a cheaper/faster model.
- **Significance**: This is the primitive that makes multi-model orchestration possible within a single agent loop iteration. Huntley's sub-agent architecture builds on this: a parent agent can oracle-out to a child, wait for a concrete plan, then execute it.

### web_search

- **Input**: `query` (required, string), `max_results` (optional, int)
- **Output**: search result list

### code_search

- **Input**: ripgrep-compatible pattern, optional file-type filters (`-t`/`-T`), optional context lines (`-B`, `-A`, `-C`), optional literal flag (`-F`)
- **Output**: matching code locations with context lines
- **Engine**: ripgrep — respects `.gitignore` by default

---

## Tool Registry Architecture

Loom uses a **trait-based registry** in the `loom-tools` crate:

```
loom-tools         — tool implementations, central registry
loom-core          — ToolCall / ToolResult type definitions, state machine
loom-llm-*         — provider adapters (Anthropic, OpenAI)
```

Each tool is a pluggable component conforming to a trait with:
- `name: String` — identifier the LLM uses in `tool_use` blocks
- `description: String` — natural language description passed in the API request
- `input_schema: JSON Schema` — parameter definitions
- `fn execute(...)` — implementation

New tools are registered by implementing the trait and inserting into the registry. No code changes to the state machine required.

---

## ToolCall / ToolResult Types

### ToolCall (LLM → Agent)

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Unique call identifier for correlation |
| `name` | string | Tool name from the registry |
| `parameters` | JSON object | Input matching the tool's schema |

### ToolResult (Agent → LLM)

| Field | Type | Description |
|-------|------|-------------|
| `tool_use_id` | string | Matches the originating `ToolCall.id` |
| `output` | string | stdout / result text |
| `error` | string? | stderr or failure message (optional) |
| `exit_code` | int? | Process exit code — bash only (optional) |

### ABA's current types (src/llm.rs)

```rust
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,  // JSON-encoded string (not parsed Value)
}

pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

// Tool results are passed back as plain String in Message { role: "tool", content, tool_call_id }
```

ABA does not yet have a dedicated `ToolResult` struct. Results are inlined into `Message.content`.

---

## Execution Flow

```
LLM generates tool_use block
  → registry lookup by name
  → tool.execute(parameters)
  → stdout/stderr/exit_code captured
  → ToolResult appended to conversation as role:"tool" message
  → LLM called again with full updated history
  → repeat until LLM returns no tool calls (max 50 turns)
  → PostToolsHook runs
```

**Parallel execution**: Loom supports configurable parallel subtasks (`max_parallel_subtasks`). Huntley's sub-agent documentation references 250–500 parallel subagents for large specification studies. ABA currently executes tools sequentially within a turn (iterating `tool_calls` with a `for` loop in `agent.rs`).

**No human-in-the-loop approval**: All tools execute automatically. Safety is enforced by the Ralph loop's fitness check (PostToolsHook), not by pre-execution gating.

---

## PostToolsHook

PostToolsHook runs **once per agent iteration**, after the LLM finishes (no more tool calls).

| Trigger condition | Which tools cause it |
|-------------------|----------------------|
| Any mutating tool was called | `bash`, `edit_file` |
| LLM finishes cleanly with no tool calls | always runs |

**Hook logic** (Loom and ABA identical in principle):
1. Run `cargo test`
2. If tests pass: auto-commit all changes
3. If tests fail: hard-reset to last committed state (discard all changes)

PostToolsHook is NOT triggered after individual tool calls — it is a single post-loop checkpoint. Read-only tools (`read_file`, `list_files`, `oracle`, `web_search`, `code_search`) do not by themselves cause a commit/revert cycle.

---

## Oracle Tool — Extended Notes

The oracle tool represents a design pattern worth preserving explicitly:

**Why it matters**: Standard agent loops are single-model. Oracle makes the tool system itself an LLM routing layer. The executing agent can ask a second model for a second opinion, delegate research to a cheaper model, or use a model with a longer context window for a subtask — all without breaking out of the current tool loop or spawning a subprocess.

**Sub-agent pattern** (Huntley's documented vision, not yet in ABA):
- Parent agent calls `oracle` with a complex planning query
- Oracle returns a concrete next-step plan
- Parent executes the plan with `bash`/`edit_file` calls
- Parent waits for sub-agent via result; no async coordination needed

**Context window cloning**: Huntley describes passing the parent's full conversation history as context to the oracle, effectively cloning the agent's working memory into the child call. This is not constrained by Loom's tool API — it is just a large `system_prompt` + `query` parameter.

---

## ABA Current State vs. Loom

| Capability | Loom | ABA (current) |
|------------|------|----------------|
| bash | Yes, with cwd + timeout params | Yes, command only, no timeout |
| edit_file | Yes | No — uses bash for all edits |
| read_file | Yes | No — uses bash |
| list_files | Yes | No — uses bash |
| oracle | Yes | No |
| web_search | Yes | No |
| code_search | Yes | No |
| Tool registry trait | Yes | No — hardcoded if/else in agent.rs |
| ToolResult struct | Yes | No — plain String |
| Parallel tool execution | Yes | No — sequential |
| PostToolsHook | Yes | Yes |
| VCS abstraction (Vcs trait) | Planned | Yes (Git + JJ) |
| Timeout on bash | 300s max | None |

---

## Key Insights for ABA

**1. The registry pattern eliminates the if/else dispatch**
ABA's `execute_tool()` in `agent.rs` is a hardcoded `if tool_call.name == "bash"` block. Loom's trait-based registry makes adding tools a one-file change with no modification to the agent loop. This is the highest-leverage structural improvement.

**2. bash alone is not sufficient for reliable edits**
Using `bash` to write files (e.g., `cat > file.rs << 'EOF'`) fails silently on heredoc edge cases and large files. Loom's `edit_file` with exact-string matching is more deterministic and produces smaller, auditable diffs. ABA should add `edit_file` as a first-class tool.

**3. Bash timeout is missing**
ABA's bash tool has no timeout. A runaway `cargo build` or infinite loop in the agent's own test suite will block the loop forever. Loom caps bash at 300 seconds. `std::process::Command` needs `tokio::time::timeout` wrapping.

**4. `cwd` parameter enables multi-project agent loops**
Loom's bash tool accepts a `cwd` parameter so the agent can operate on subdirectories (e.g., `dummy-project/`) without `cd` in every command. ABA's Ralph loop currently uses `dummy-project/` for tests — adding `cwd` support would make this cleaner.

**5. Oracle is the key to multi-model loops**
The oracle tool is Loom's mechanism for multi-model orchestration. ABA's `LlmClient` trait is already provider-agnostic; adding an oracle tool means instantiating a second `LlmClient` inside the tool and calling `.complete()`. The infrastructure is already there.

**6. ToolResult as a typed struct improves observability**
ABA serializes tool results as raw strings. A typed `ToolResult { tool_use_id, output, error, exit_code }` struct would allow structured logging, tracing, and future retry logic on specific error categories.

**7. PostToolsHook always runs — not only on mutation**
Both Loom and ABA run the hook unconditionally at loop end. If the LLM only read files and then finished, the hook still runs `cargo test`. This is intentional (ensures committed state is always valid) but means a no-op iteration still pays the `cargo test` cost. An optimization: track whether any mutating tool was called and skip the hook if not.

**8. No human-in-the-loop is a deliberate philosophy**
Huntley's design explicitly avoids pre-execution approval gates. The Ralph loop's safety model is: iterate rapidly, revert automatically on failure, rely on `cargo test` as the fitness function. This is the correct model for ABA's use case.
