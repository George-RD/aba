# Source Control System

## Overview

ABA's agent loop requires a version control backend for two purposes:
1. **Committing successful work** — when tests pass after tool execution
2. **Reverting failed work** — when tests fail, roll back to a known-good state

The current implementation uses git directly via `std::process::Command`. This spec defines a VCS abstraction layer that starts with git and is designed to swap to Jujutsu (JJ) as the primary backend.

## Why JJ Over Git

Git was designed for human workflows. Several of its design choices actively hurt autonomous agent loops:

| Git Problem | Impact on Agent Loop | JJ Solution |
|-------------|---------------------|-------------|
| Staging area (`git add`) | Unnecessary step before every commit; agent must always `add -A` | Working copy IS a commit; every file write is automatically tracked |
| `git reset --hard` is destructive | Failed agent attempts are permanently lost; no way to learn from them | `jj undo` reverts while preserving the failed attempt in the operation log |
| Conflicts block operations | A rebase that conflicts halts and waits for interactive resolution | Conflicts stored as data inside commits; operations always succeed |
| Lock files on concurrent ops | Two agents running `git commit` simultaneously race and one fails | Lock-free concurrency model; multiple agents can operate simultaneously |
| Branch naming required | Creating experimental changes requires naming branches upfront | Anonymous changes tracked by change ID; bookmarks are optional labels |
| History rewriting is dangerous | `git rebase -i` and `git commit --amend` can lose data | Every mutation recorded in operation log; any state restorable |

### Key JJ features for ABA

1. **Operation log** — append-only history of every repo mutation. Failed agent runs become inspectable data. A supervisor agent could `jj op log` to audit what happened and `jj op restore` to any prior state.

2. **Automatic snapshotting** — running any `jj` command snapshots the working copy first. Combined with the op log, this creates implicit checkpoints around every tool execution.

3. **Workspaces** — `jj workspace add` creates filesystem-isolated copies sharing the same repo. Each agent (weaver) can get its own workspace with zero coordination overhead. This is the foundation for multi-agent parallel work.

4. **Git compatibility** — `jj git init --colocate` works inside an existing git repo. Both systems see the same commits. Adoption is incremental; collaborators using git see normal git history.

## Architecture

### VCS Trait

```rust
#[async_trait]
pub trait Vcs: Send + Sync {
    /// Commit all current changes with the given message.
    fn commit_all(&self, message: &str) -> Result<(), VcsError>;

    /// Revert to the last committed state.
    /// For git: `git reset --hard`
    /// For JJ: `jj undo` (preserves history in op log)
    fn revert(&self) -> Result<(), VcsError>;

    /// Get a summary of current changes (for commit message generation).
    fn status(&self) -> Result<String, VcsError>;
}
```

### Git Backend (current)

Wraps the existing `tools.rs` functions behind the `Vcs` trait. Behavior unchanged:
- `commit_all` → `git add -A && git commit -m <message>`
- `revert` → `git reset --hard`
- `status` → `git status --short`

### JJ Backend (target)

Uses the `jj` CLI (not jj-lib as a Rust dependency — keep it simple, shell out like git):
- `commit_all` → `jj commit -m <message>` (no staging step needed)
- `revert` → `jj undo` (non-destructive; attempt preserved in op log)
- `status` → `jj status`

Note: Loom's "Spool" wraps jj-lib as a Rust crate, but this adds complexity without functional benefit — Spool is a 1:1 API rename of stock jj-lib 0.28 with no agent-specific features actually implemented. For ABA, shelling out to the `jj` CLI is simpler and sufficient.

### Backend Selection

Detect which VCS is available in the workspace:
1. If `.jj/` exists → use JJ backend
2. If `.git/` exists → use git backend
3. If both exist (colocated) → prefer JJ

This can also be overridden via config (`AbaConfig.vcs_backend: Option<String>`).

## Implementation Plan

### Phase 1: Extract VCS trait from current code
- Move `git_commit_all()` and `git_reset_hard()` from `tools.rs` into a `Vcs` trait
- Create `GitVcs` struct implementing the trait
- Update `agent.rs` PostToolsHook to use the trait instead of calling git functions directly
- No behavior change; pure refactor

### Phase 2: Add JJ backend
- Create `JjVcs` struct implementing the `Vcs` trait
- Shell out to `jj` CLI (same pattern as git)
- Add workspace detection logic
- Add `vcs_backend` to `AbaConfig`

### Phase 3: Enhanced JJ features
- Add `Vcs::snapshot()` method for explicit checkpointing before risky tool calls
- Add `Vcs::operation_log()` for the agent to inspect its own history
- Add workspace management for multi-agent support (`jj workspace add`)

## Non-Goals

- Forking JJ or renaming its commands (Loom's Spool approach adds complexity without function)
- Using jj-lib as a Rust dependency (CLI is simpler, avoids version coupling)
- Replacing git entirely (colocated mode keeps both working)
