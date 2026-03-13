# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is ABA?

ABA (Agent Builds Agent) is a Rust CLI tool that acts as a self-improving coding agent. Inspired by Geoffrey Huntley's "Loom" project and the **Ralph Wiggum Loop** philosophy: instead of interactive AI coding (Cursor-style back-and-forth), program simple single-objective loops that iterate until a goal is achieved. Engineers act as managers orchestrating agent loops, not manually guiding individual agents.

ABA implements the core Ralph loop: read a prompt from stdin → multi-turn LLM conversation with tool calls → `cargo test` as fitness check → auto-commit on success, hard-reset on failure.

## Build & Test Commands

```bash
cargo build              # compile
cargo test               # run all tests
cargo run                # run (expects stdin; see usage below)
cargo test -- test_name  # run a single test by name
cargo clippy             # lint
cargo fmt                # format
```

### Running the agent

The agent reads its prompt from stdin:
```bash
cat PROMPT_build.md | cargo run
```

### Running the Ralph loop

```bash
./loop.sh              # Build mode, unlimited iterations
./loop.sh 20           # Build mode, max 20 iterations
./loop.sh plan         # Plan mode (generates IMPLEMENTATION_PLAN.md)
./loop.sh plan 5       # Plan mode, max 5 iterations
```

### Auth configuration

On first run with no config, an interactive setup prompts for auth method (OpenAI OAuth, OpenAI API key, or Anthropic API key). Config is persisted to `~/.config/ABA/config.toml`.

Auth can also be set via environment variables: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, or `OAUTH_CLIENT_ID`.

## Architecture

Single Cargo package (edition 2024) with four modules:

- **`agent.rs`** — Multi-turn agent loop. Calls LLM → executes tool calls → feeds results back → repeats until LLM finishes. Then runs PostToolsHook (`cargo test` → commit/revert). Max 50 tool turns per iteration.
- **`llm.rs`** — `LlmClient` trait with `AnthropicClient` (Messages API, tool_use parsing) and `OpenAiOAuthClient` (Chat Completions, function calling, optional OAuth device flow). Types: `Message`, `ToolCall`, `ToolDefinition`, `LlmRequest`, `LlmResponse`.
- **`tools.rs`** — `Vcs` trait (commit_all, revert, status) with `GitVcs` implementation. `bash_tool_definition()` returns the tool schema for the LLM. Backward-compatible `git_commit_all()` and `git_reset_hard()` functions.
- **`config.rs`** — `AbaConfig` TOML config via the `directories` crate.
- **`main.rs`** — CLI entry point. Auth setup, LLM client selection, reads stdin, launches agent.

### Ralph Loop Files

- `loop.sh` — Outer bash loop (plan/build modes, max iterations, git push per iteration)
- `PROMPT_plan.md` — Planning mode: gap analysis, generates `IMPLEMENTATION_PLAN.md`
- `PROMPT_build.md` — Build mode: pick task from plan, implement, test, commit

### Specs

- `specs/agent-core.md` — Agent loop, tool system, LLM abstraction
- `specs/source-control.md` — VCS trait, git→JJ migration path
- `specs/self-bootstrapping.md` — What ABA should build when Ralph-looping on itself

### `dummy-project/`

Minimal Rust project with an intentionally failing test (`assert_eq!(2 + 2, 5)`). Test target for the agent.
