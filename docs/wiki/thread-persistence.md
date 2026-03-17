# Loom Thread Persistence & Search

Source: `docs/research/loom-thread-persistence-search.md` (compressed).
Reference: github.com/ghuntley/loom

---

## Thread Data Model

**ThreadId**: UUID7 with `T-` prefix (e.g. `T-[UUID7]`). Uses the `uuid7` crate.

### Full Thread Fields

| Field | Type | Notes |
|---|---|---|
| `thread_id` | `ThreadId` (UUID7 with T- prefix) | Primary identifier |
| `version` | `i64` | Optimistic locking counter, incremented on each update |
| `conversation` | `ConversationSnapshot` | Full message array with roles, content, timestamps, tool calls |
| `agent_state` | `AgentStateSnapshot` | Tool execution state, memory context, reasoning traces |
| `workspace` | `WorkspaceSnapshot` | Repo info, branch, file context, model/temperature settings |
| `title` | `TEXT` | Human-readable thread title |
| `tags` | `Vec<String>` | User-assigned tags for categorisation |
| `message_count` | `u32` | Count of conversation messages (denormalised) |
| `metadata` | `serde_json::Value` | Arbitrary extra fields, schema-free |
| `visibility` | `ThreadVisibility` | `Organization` \| `Private` \| `Public` |
| `is_private` | `bool` | If true, thread MUST NEVER be sent to the server |
| `is_shared_with_support` | `bool` | Opt-in support sharing, independent of visibility |
| `created_at` | `TIMESTAMP` | ISO 8601 creation time |
| `updated_at` | `TIMESTAMP` | ISO 8601 last modification time |
| `last_activity_at` | `TIMESTAMP` | Last conversation activity; used as recency tiebreaker in search |
| `deleted_at` | `TIMESTAMP NULL` | Server soft-delete; NULL until deleted |
| `workspace_id` | FK | Associated workspace |
| `created_by` | FK → users | |
| `updated_by` | FK → users | |

### ThreadSummary (lightweight listing view)

Omits `conversation_json`, `agent_state_json`, `metadata_json`. Includes: `thread_id`, `workspace_id`, `title`, `created_at`, `updated_at`, `last_activity_at`, `message_count`, `visibility`, `tags`, `is_private`, `is_shared_with_support`.

### ThreadVisibility Enum

- `Organization` — visible to org members; syncs to server
- `Private` — local-only; never syncs (enforced by `is_private=true` invariant)
- `Public` — globally visible, opt-in

---

## Local Storage

**XDG-compliant paths** (via `directories` crate):

- Thread data: `$XDG_DATA_HOME/loom/threads/{thread_id}.json` (typically `~/.local/share/loom/threads/`)
- Pending sync queue: `$XDG_STATE_HOME/loom/sync/pending.json` (typically `~/.local/state/loom/sync/`)

**Atomic write pattern**: write to temp file → atomic rename. Prevents corruption from partial writes.

---

## Server-Side Storage

**SQLite with WAL mode** for concurrent reads. One writer at a time (SQLite constraint). Uses `sqlx` 0.8 as async executor. Migrations in `crates/loom-server/migrations/` (`NNN_description.sql`; inline SQL in other crates is prohibited).

### threads Table (denormalised)

```sql
CREATE TABLE threads (
  thread_id          TEXT PRIMARY KEY,  -- UUID7 with T- prefix
  workspace_id       TEXT NOT NULL,
  version            INTEGER NOT NULL,  -- optimistic locking
  title              TEXT NOT NULL,
  deleted_at         TIMESTAMP,         -- NULL = active; soft-delete
  is_private         BOOLEAN NOT NULL,
  is_shared_with_support BOOLEAN NOT NULL,
  created_at         TIMESTAMP NOT NULL,
  updated_at         TIMESTAMP NOT NULL,
  last_activity_at   TIMESTAMP NOT NULL,
  message_count      INTEGER NOT NULL,
  conversation_json  TEXT NOT NULL,
  agent_state_json   TEXT NOT NULL,
  metadata_json      TEXT NOT NULL,
  visibility         TEXT NOT NULL,     -- Organization|Private|Public
  tags               TEXT NOT NULL,     -- JSON array
  created_by         TEXT NOT NULL,
  updated_by         TEXT NOT NULL
);
```

### FTS5 Search Index

```sql
CREATE VIRTUAL TABLE thread_fts USING fts5(
  thread_id    UNINDEXED,
  title,
  content,        -- full message text from conversation
  git_commit,     -- commit SHA (prefix-searchable)
  git_branch,
  git_repo_url,
  tags,
  tokenize = 'unicode61'
);
```

Three database triggers keep `thread_fts` in sync with `threads`: INSERT, UPDATE, DELETE.

**Ranking**: BM25 relevance (`bm25()` built-in), tiebroken by `last_activity_at DESC`.

---

## ThreadStore Trait

```rust
trait ThreadStore {
    async fn load(&self, thread_id: &ThreadId) -> Result<Thread>;
    async fn save(&self, thread: &Thread) -> Result<()>;
    async fn list(&self) -> Result<Vec<Thread>>;
    async fn delete(&self, thread_id: &ThreadId) -> Result<()>;
}
```

Three implementations:

- `LocalThreadStore` — reads/writes JSON files under XDG data dir
- `SyncingThreadStore` — wraps `LocalThreadStore`; queues background sync; enforces `is_private` invariant
- `ThreadSyncClient` — REST client for server; adds `upsert_thread`, `get_thread`, `list_threads` (returns `ThreadSummary`), `delete_thread`

---

## Sync Architecture (Local-First)

**SyncingThreadStore** is the primary client-facing store:

```
Thread operation
  → Save to LocalThreadStore (always)
  → is_private = true? → STOP (never sync)
  → Queue for background sync
      → Success: remove from queue
      → Failure: persist to PendingSyncStore
          → Retry with exponential backoff
```

**Sync is triggered at:**
1. After inferencing (agent returns to `WaitingForUserInput`)
2. Graceful shutdown (SIGINT / stdin EOF / explicit exit)

### PendingSyncStore

File: `$XDG_STATE_HOME/loom/sync/pending.json`

```json
{
  "pending_syncs": [{
    "thread_id": "T-[UUID7]",
    "operation": "upsert|delete",
    "thread_data": {},
    "attempts": 3,
    "last_attempt_at": "2026-03-17T10:30:00Z",
    "next_retry_at": "2026-03-17T10:35:00Z",
    "error_message": "Connection timeout"
  }]
}
```

Persists across process restarts. Exponential backoff between retries.

---

## REST API Endpoints

Base: `/v1/threads/`

| Method | Path | Description | Success | Error |
|---|---|---|---|---|
| `POST/PUT` | `/v1/threads/` | Upsert thread | 200 (updated) / 201 (created) | 409 version mismatch |
| `GET` | `/v1/threads/{id}` | Fetch full thread | 200 | 404 not found |
| `GET` | `/v1/threads/?workspace={id}&limit={n}&offset={n}` | List threads (paginated, returns ThreadSummary) | 200 | — |
| `DELETE` | `/v1/threads/{id}` | Soft-delete (sets `deleted_at`) | 204 | — |
| `GET` | `/v1/threads/search?q={query}&workspace={id}&limit={n}&offset={n}` | FTS5 search, ranked by BM25 | 200 JSON array | — |

---

## Optimistic Concurrency Control

**If-Match version header pattern:**

```
1. Client fetches thread → version = 5
2. Concurrent update occurs → server version becomes 6
3. Original client sends update with version = 5
4. Server rejects: 409 Conflict
5. Client re-fetches (version = 6), merges, retries
```

- No pessimistic locking; no blocking
- `version` incremented on every successful write
- Clients are responsible for conflict resolution and retry

---

## Search

**Server path**: FTS5 query via `GET /v1/threads/search`.

**Supported query types**: keyword, commit SHA prefix, branch name, repo URL, tag, title.

**CLI**: `loom search <query> [--limit n] [--offset n] [--workspace id] [--json]`

**Offline fallback**: local substring scan across titles, git fields, tags, message content — no BM25 ranking.

---

## Crate Layout

- `loom-thread` — `Thread`, `ThreadId`, `ThreadVisibility`, `LocalThreadStore`, `SyncingThreadStore`
- `loom-server` — HTTP API, SQLite, `ThreadSyncClient`, FTS5 management, migrations

---

## Key Insights for ABA

1. **UUID7 for sortable IDs.** UUID7 embeds a timestamp, making thread IDs lexicographically sortable by creation time — useful for listing without an extra sort column. ABA could adopt `T-[UUID7]` as a convention for agent run IDs.

2. **`is_private` as a hard invariant, not a flag.** Loom treats `is_private=true` as an absolute local-only contract enforced at the store layer, not a preference. ABA could mirror this for runs that must not leave the machine (sensitive codebases, air-gapped environments).

3. **Sync triggers on state transitions, not timers.** Sync fires after the agent idles (`WaitingForUserInput`) and on shutdown — not on a polling interval. This avoids mid-turn partial state reaching the server. ABA's Ralph loop has a natural equivalent: sync after each `cargo test` pass.

4. **PendingSyncStore decouples durability from connectivity.** The retry queue is itself a persistent file, so a crash mid-sync does not lose the pending operation. ABA's post-`cargo test` commit step could use the same pattern: write intent to disk before executing the git operation.

5. **FTS5 `git_commit` field.** Indexing the commit SHA directly in the search index enables fuzzy discovery of threads by prefix-matching commit hashes. For ABA, indexing the commit SHA in each agent run record would allow "find the run that produced commit `abc123`" queries.

6. **`last_activity_at` as a separate field from `updated_at`.** Loom distinguishes metadata edits (`updated_at`) from conversation activity (`last_activity_at`). ABA could track a similar `last_tool_call_at` to rank runs by actual work done rather than admin changes.

7. **Denormalisation is intentional.** Fields like `message_count`, `tags`, and `workspace_id` are duplicated out of JSON blobs into real columns for query efficiency. For ABA, tool-call count and exit status should be first-class columns, not buried in a JSON field.

8. **Soft-delete as the only server delete.** Hard deletes only happen locally. The server preserves the audit trail indefinitely. For ABA, preserving all run records (even failed ones) is consistent with the Ralph loop philosophy of iterative improvement — you need to inspect past failures.
