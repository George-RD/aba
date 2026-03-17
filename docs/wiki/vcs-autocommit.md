# VCS & Auto-Commit Reference

Source: Geoffrey Huntley's [ghuntley/loom](https://github.com/ghuntley/loom) (trunk branch).
Compiled from research in `docs/research/loom-vcs-spool-autocommit.md`.

---

## Spool System Overview

**Spool** is Loom's VCS layer, built on top of Jujutsu (jj). It is not a jj-lib fork or a renamed wrapper — it is a full Jujutsu implementation that adds textile-themed semantics, agent-specific optimizations, and colocated Git interoperability. The textile terminology aligns with the "Loom" metaphor: weaving threads of work into a codebase.

### Textile Terminology Table

| Git Concept | Jujutsu Term | Spool Term | Meaning |
|---|---|---|---|
| Anonymous change | Working copy | **Stitch** | Atomic unit of work; no commit message required |
| Committed change | Change | **Knot** | Stitch with a message attached |
| Working directory | Workspace | **Shuttle** | Active working context |
| Rebase operation | Rebase | **Rethreading** | Auto-rebase when pulling upstream changes |
| Merge conflict | Conflict | **Tangle** | Explicit conflict state requiring resolution |
| Operation log entry | Op log | **Tension log** / **Unpick** | Record of every VCS operation; supports undo |
| Checkout | Check out | **Draw** | Switch working context |

### Relationship to Stock jj

Spool is NOT simply jj-lib renamed. The distinction:

- **jj-lib** is the Rust library crate extracted from Jujutsu for embedding in other tools.
- **Spool** builds on the full Jujutsu codebase and extends it with:
  - Textile UX semantics (stitch/knot/shuttle nomenclature)
  - Agent-specific auto-stitch creation after tool executions
  - Colocated `.spool/` + `.git/` directories for GitHub interoperability
  - Tension log integration for agent audit trails

Formula: **Loom Spool = Jujutsu + textile UX + agent optimizations**

### Spool Crates

| Crate | Role |
|---|---|
| `loom-common-spool` | Core library; stitch/knot/shuttle abstractions, operation tracking |
| `loom-cli-spool` | CLI interface; rethreading, tangle resolution, unpick |
| `loom-server-spool` | (planned) Remote hosting for distributed Spool instances |

---

## Tension Log (Operation Log)

The **tension log** is Spool's equivalent of Jujutsu's operation log. It records every VCS operation with full metadata, enabling complete audit trails and recovery.

**Structure of each entry:**
- Unique operation ID
- Timestamp
- "View object" — snapshot of the entire repo state at that point (branches, tags, working-copy commits)

**Commands:**
```bash
jj op log                    # view the tension log
jj op restore <op-id>        # restore repo to an earlier state (unpick)
```

**Why it matters for agents:** If an agent corrupts the repository, the tension log provides a complete, time-ordered audit trail. Any state is recoverable. Agents can be queried "what changed in thread X?" because every operation is tagged with thread/agent metadata.

**In textile terms:** "tension" refers to the pull and weave of threads. The log captures the tension of each operation that wove the repository into its current state.

---

## GitClient Trait (loom-git)

`loom-git` provides a testable Git abstraction layer. All operations are scoped to the workspace root and rejected outside a git repository.

```rust
trait GitClient {
    async fn diff_staged(&self) -> Result<String>;
    async fn diff_unstaged(&self) -> Result<String>;
    async fn changed_files(&self) -> Result<Vec<String>>;
    async fn stage(&self, paths: Vec<String>) -> Result<()>;
    async fn commit(&self, message: String) -> Result<CommitHash>;
    async fn status(&self) -> Result<RepoStatus>;
}
```

**Implementations:**
- `CommandGitClient` — shells out to the git CLI
- `MockGitClient` — test double with builder patterns for reproducible test scenarios

---

## Auto-Commit PostToolsHook Flow

The auto-commit system is infrastructure — invisible to the LLM. The agent never sees commits happen; it just continues its conversation.

### State Machine Position

```
ExecutingTools
    ↓  (all tools complete; mutations detected)
PostToolsHook  ←── auto-commit fires here
    ↓  (PostToolsHookCompleted event)
CallingLlm
```

### Step-by-Step Flow

1. One or more tools finish executing in `ExecutingTools`.
2. `loom-git.changed_files()` is called — if the returned vec is non-empty, mutations are detected.
3. State machine transitions to `PostToolsHook`, carrying: conversation context, pending LLM request, and completed tool info.
4. `AutoCommitService` runs:
   a. Checks `LOOM_AUTO_COMMIT_DISABLE` env var (skip if set).
   b. Reads changed files and measures diff size.
   c. If diff ≤ 32KB: passes full diff to `CommitMessageGenerator`.
   d. If diff > 32KB: truncates to 32KB, sets `truncated = true`.
   e. `CommitMessageGenerator` calls Claude Haiku to produce a conventional commit message.
   f. Stages all changed files via `GitClient.stage()`.
   g. Creates the commit via `GitClient.commit(message)`.
   h. Returns `AutoCommitResult`.
5. `PostToolsHookCompleted` event fires with result metadata.
6. State machine transitions to `CallingLlm` to resume the conversation.

### Mutating Tools (Auto-Commit Triggers)

Auto-commit fires only when `changed_files()` returns a non-empty list. In practice:

| Tool | Mutates files? | Triggers auto-commit? |
|---|---|---|
| `read_file` | No | No |
| `list_files` | No | No |
| `edit_file` | Yes | Yes |
| `bash` | Yes (if writes) | Yes |
| `oracle` | No | No |
| `web_search` | No | No |

---

## Haiku Commit Message Generation

### Prompt and Format

Claude Haiku (fast, low-cost model) is given the diff and produces a **conventional commit** message:

```
<type>(<optional-scope>): <description>

<optional-body>

<optional-footer>
```

**Type inference from changed files:**

| Scenario | Type |
|---|---|
| New file in `src/` | `feat:` |
| Modified file in `src/` | `fix:` or `refactor:` (Haiku infers) |
| New or modified test file | `test:` |
| Documentation | `docs:` |
| Build/config files | `chore:` |

**Scope:** derived from the file path (e.g., `auth`, `api`, `tools`).

**Description:** kept under 72 characters.

### 32KB Truncation

When the diff exceeds 32KB:
- Diff is truncated to 32KB.
- Commit message includes file names and statistics.
- A truncation notice is appended to the message body.
- The commit still proceeds normally.
- `AutoCommitResult.truncated` is set to `true`.

### Fallback Message

When Haiku is unavailable (timeout, network failure, API error):
```
chore: auto-commit from loom [Auto-generated fallback: LLM unavailable]
```

---

## AutoCommitResult Type

```rust
struct AutoCommitResult {
    committed: bool,
    commit_hash: Option<CommitHash>,
    message: String,
    files_changed: usize,
    total_diff_size: usize,
    truncated: bool,
}
```

---

## Fail-Open Behavior

The auto-commit system **never blocks the agent**. Every error path logs details and allows the agent loop to continue:

| Failure | Behavior |
|---|---|
| `GitClient` error | Log, skip commit, agent continues |
| Haiku API timeout | Use fallback message, commit with fallback |
| Staging fails | Log, skip commit, agent continues |
| Network issue | Brief retry window, then fallback message |
| `LOOM_AUTO_COMMIT_DISABLE=1` | Skip silently, agent continues |

---

## Back-Pressure Philosophy

Loom agents push directly to trunk with no pull requests, no code review, and no human approval gates. The safety net is automated:

### Safety Net Layers

1. Unit tests — verify module-level functionality
2. Integration tests — verify subsystem interactions
3. Property-based tests — verify invariants across inputs
4. `cargo clippy` — catch common Rust mistakes
5. `cargo fmt` — enforce code style
6. Build verification — confirm compilation succeeds

### Failure Response

When CI fails on a trunk push:
1. Commit is reverted automatically.
2. Agent re-loops: picks up the task again from the prompt.
3. Each iteration has a chance to fix what the previous iteration broke.

### Philosophy

Traditional: `Engineer writes → Human review → Tests → Deploy`
Ralph loop: `Agent writes → Tests → Pass: deploy / Fail: re-loop`

Geoffrey Huntley's argument: iterative agent loops converge to correct solutions faster than waiting for human review cycles, and become safer as LLM quality improves.

```
git push origin trunk
    ↓
CI pipeline (tests + lint + build)
    ↓
Pass → auto-deploy (10-second NixOS poller detects new commit)
Fail → revert + agent re-loops
```

---

## Key Insights for ABA

1. **Spool is not just renamed jj.** It extends jj with textile semantics, agent-specific auto-stitch creation, and colocated Git mode. The distinction matters: ABA should use the jj CLI directly (behind a `Vcs` trait) rather than trying to embed jj-lib.

2. **PostToolsHook pattern matches ABA's existing architecture.** ABA already has a `PostToolsHook` concept (`cargo test` after tool calls). Loom shows that auto-commit should slot in at the same point — after tool execution, before the next LLM turn.

3. **GitClient trait is the right abstraction boundary.** Separating `diff_staged`, `diff_unstaged`, `changed_files`, `stage`, and `commit` into a trait makes the auto-commit logic testable without real git repos. ABA should adopt this pattern.

4. **Fail-open is not optional.** Auto-commit errors must never block the agent loop. If a commit fails, log it and continue. The agent's primary job is coding, not VCS bookkeeping.

5. **32KB truncation prevents context blowup.** Without a size limit, large diffs in commit messages (or in LLM context) become expensive and unreliable. Hard cap at 32KB with a truncation flag is the right default.

6. **Haiku for commit messages, not the primary model.** Using the cheapest capable model for routine commit message generation reduces cost significantly in long Ralph loops. ABA could use Haiku (or equivalent) for this specific sub-task.

7. **NEW: Tension log = agent undo primitive.** The operation log is not just audit logging — it is the recovery mechanism. If ABA adopts jj (or Spool-style ops logging), it gains the ability to roll back any agent-caused change precisely, without needing a branch-based revert workflow.

8. **NEW: Auto-stitch creates fine-grained history within a task.** Each tool execution that mutates files creates a commit. This means a Ralph loop iteration that calls `edit_file` 10 times produces 10 commits — a full step-by-step record of the agent's reasoning process in code. This is significantly more valuable for debugging than a single squash commit per iteration.

9. **NEW: Colocated `.spool/` + `.git/` solves the GitHub compatibility problem.** If ABA moves to jj, it can use jj's colocated mode to maintain a `.git/` directory alongside jj's state. GitHub, CI systems, and existing tooling all continue to work without modification.
