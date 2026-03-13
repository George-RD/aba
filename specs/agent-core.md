# Agent Core System

## Overview

The agent core is a multi-turn tool-calling loop that receives a prompt, iterates with an LLM until the task is complete, then runs a fitness check (tests pass → commit, tests fail → revert).

## Architecture

### State Machine (simplified from original)

```
Prompt (stdin) → [Multi-turn LLM ↔ Tool loop] → PostToolsHook → Exit
```

The original state machine (`WaitingForUserInput → CallingLlm → ExecutingTools → PostToolsHook → ShuttingDown`) was overly complex for a headless agent that reads from stdin. The simplified flow is:

1. Read prompt from stdin
2. Call LLM with prompt + tool definitions
3. If LLM returns tool calls: execute them, feed results back, goto 2
4. If LLM returns no tool calls: task complete, run PostToolsHook
5. PostToolsHook: `cargo test` → pass: commit / fail: revert
6. Exit

### Multi-Turn Conversation

The agent maintains a `Vec<Message>` conversation history across the inner loop. Each turn:
- Sends full conversation + tool definitions to the LLM
- LLM responds with text and/or tool calls
- Assistant message (with tool calls) appended to history
- Tool results appended as tool-result messages
- Next LLM call sees the full history

A `MAX_TOOL_TURNS` safety limit prevents infinite loops.

### LLM Abstraction

`LlmClient` trait with two backends:
- `AnthropicClient` — Anthropic Messages API (tool_use content blocks)
- `OpenAiOAuthClient` — OpenAI Chat Completions API (function calling), with optional OAuth device flow

Both accept `LlmRequest { system_prompt, messages, tools, max_tokens, temperature }` and return `LlmResponse { text, tool_calls }`.

### Tool System

Tools are defined as `ToolDefinition { name, description, input_schema }` and sent to the LLM in each request. Currently only `bash` is implemented.

Tool execution is synchronous (one tool at a time). Results include stdout, stderr, and exit code.

#### Future tools (for ABA to build itself)
- `read_file` — read file contents (avoids bash cat overhead, better error handling)
- `edit_file` — structured find/replace (avoids sed/awk, less error-prone for the LLM)
- `list_files` — directory listing with glob support
- `code_search` — ripgrep-powered pattern search

These match the tool progression from Geoffrey Huntley's how-to-build-a-coding-agent workshop.

### PostToolsHook

Runs after the LLM finishes (no more tool calls). Currently:
1. `cargo test` — fitness check
2. Pass → `git_commit_all()` (via VCS trait)
3. Fail → `git_reset_hard()` (via VCS trait)

Future: configurable test command, LLM-generated commit messages, partial commit support.

### Observability

Every tool execution is logged via `tracing` (info for success, warn for stderr, error for failures). The outer Ralph loop captures all output.

Future observability (inspired by Loom):
- Structured JSON output for each tool call (tool name, args, result, duration)
- Thread/audit trail file written per iteration (reusable as context for future runs)
- Metrics: tokens used, tool calls made, test pass/fail, time per turn
