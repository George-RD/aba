# LLM Proxy & Provider Integration

Reference for Geoffrey Huntley's Loom project. Source: `docs/research/loom-llm-proxy-providers.md`.

---

## Architecture

### Core Principle

API keys are stored server-side only. Clients (`loom-cli`, weaver pods) authenticate with a bearer token; `loom-server` fetches the real provider credential before proxying.

### Crate Layout

| Crate | Purpose |
|---|---|
| `loom-common-core/llm.rs` | `LlmClient` trait, `LlmRequest`, `LlmResponse`, `Usage` |
| `loom-server-llm-proxy/` | `ProxyLlmClient` + SSE stream parser (client side) |
| `loom-server-llm-service/` | `LlmService` — server-side provider orchestration |
| `loom-server-llm-anthropic/` | `AnthropicClient`, `AnthropicPool`, OAuth auth |
| `loom-server-llm-openai/` | `OpenAIClient`, OpenAI streaming |
| `loom-server-llm-vertex/` | Google Vertex AI support |
| `loom-server-llm-zai/` | Z.ai provider support |
| `loom-server/routes/llm_proxy.rs` | HTTP route handlers |
| `loom-server/query_security/` | `RateLimiter`, `PathSanitizer`, `QueryValidator` |

### Request Flow

```
loom-cli / weaver pod
  │
  └─ POST /proxy/{provider}/complete|stream
     Authorization: Bearer {auth_token}
       │
       └─ loom-server
            ├─ Validates auth token
            ├─ Fetches provider credential (CredentialsManager)
            ├─ Applies rate limiting / query validation
            └─ Forwards to LLM provider API
                 └─ Returns SSE or JSON response to client
```

---

## HTTP Endpoints

### LLM Proxy

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/proxy/anthropic/complete` | Non-streaming Anthropic completion |
| `POST` | `/proxy/anthropic/stream` | SSE streaming Anthropic completion |
| `POST` | `/proxy/openai/complete` | Non-streaming OpenAI completion |
| `POST` | `/proxy/openai/stream` | SSE streaming OpenAI completion |
| `POST` | `/proxy/vertex/*` | Vertex AI (same pattern) |
| `POST` | `/proxy/zai/*` | Z.ai provider |
| `POST` | `/proxy/cse` | Google Custom Search utility |

All proxy endpoints require `Authorization: Bearer {auth_token}` and `Content-Type: application/json`.

### Infrastructure

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/health` | Component health + Anthropic pool status |
| `GET` | `/metrics` | Prometheus metrics |
| `GET` | `/bin/{platform}` | Platform binary download |
| `GET` | `/v1/threads` | List conversation threads |
| `GET` | `/v1/threads/{id}` | Get thread |
| `PUT` | `/v1/threads/{id}` | Update thread |
| `DELETE` | `/v1/threads/{id}` | Delete thread |
| `GET` | `/v1/threads/search?q=` | Full-text thread search |
| `GET` | `/v1/auth/oauth/anthropic/initiate` | Start OAuth device flow |
| `GET` | `/v1/auth/oauth/anthropic/token?device_code=` | Poll for OAuth token |
| `POST` | `/proxy/{provider}/query/{query_id}` | Respond to server-initiated query |

### HTTP Status Codes

| Code | Meaning |
|---|---|
| `200` | Success |
| `401` | Authentication failure (bad bearer token) |
| `403` | Authorization failure |
| `429` | Rate limited — triggers `CoolingDown` in `AnthropicPool` |
| `500` | Server error |

---

## Core Types

### LlmClient Trait

```rust
#[async_trait]
pub trait LlmClient {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;
    async fn complete_streaming(&self, request: LlmRequest) -> Result<LlmStream, LlmError>;
}
```

All provider clients (`AnthropicClient`, `OpenAIClient`, `ProxyLlmClient`) implement this trait.

### LlmRequest

```rust
pub struct LlmRequest {
    pub model: String,              // e.g. "claude-3-5-sonnet-20241022"
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub temperature: f32,           // 0.0–2.0
    pub system: Option<String>,
    pub tools: Vec<ToolDefinition>,
}
```

Supports builder pattern: `.with_messages()`, `.with_temperature()`, `.with_tools()`.
Passing `"model": "default"` causes `LlmService` to substitute the provider default.

### LlmResponse

```rust
pub struct LlmResponse {
    pub message: Message,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub finish_reason: Option<String>,  // "end_turn", "max_tokens", "tool_calls"
}
```

### Usage (Token Tracking)

```rust
pub struct Usage {
    pub input_tokens: u32,   // Prompt tokens consumed
    pub output_tokens: u32,  // Generated tokens produced
}
// impl: total_tokens() -> u32
```

### Message / ContentBlock

```rust
pub struct Message {
    pub role: String,               // "user" | "assistant"
    pub content: Vec<ContentBlock>,
}

pub enum ContentBlock {
    Text(String),
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}
```

### ToolDefinition / ToolCall

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,  // JSON Schema
}

pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,     // Parsed JSON
}
```

### LlmEvent (Streaming)

```rust
pub enum LlmEvent {
    TextDelta { content: String },
    ToolCallDelta { call_id: String, tool_name: String, arguments_fragment: String },
    Completed { message: Message, tool_calls: Vec<ToolCall>, usage: Option<Usage> },
    Error { message: String },
    ServerQuery { query: ServerQuery },   // Server-initiated question mid-stream
}
```

### LlmStream

```rust
pub struct LlmStream {
    inner: Pin<Box<dyn Stream<Item = Result<LlmEvent, LlmError>> + Send>>,
}
// Implements Stream<Item = Result<LlmEvent, LlmError>>
// Convenience: .next() -> Option<Result<LlmEvent, LlmError>>
```

### LlmError

```rust
pub enum LlmError {
    AuthenticationFailed,
    RateLimited { retry_after_secs: u64 },
    TransientError(&'static str),   // Network, timeout, 5xx
    QuotaExceeded,
    InvalidRequest,
    ModelNotFound,
    TokenLimitExceeded,
    ApiError(String),
    ParseError(String),
    Unknown,
    NoAccountsAvailable,
}
```

---

## ProxyLlmClient (loom-cli Side)

**Location**: `crates/loom-server-llm-proxy/`

```rust
pub struct ProxyLlmClient {
    base_url: String,
    provider: LlmProvider,
    http_client: Client,
    auth_token: Option<String>,
}

pub enum LlmProvider { Anthropic, OpenAI, VertexAI }
```

**Factory methods**: `ProxyLlmClient::anthropic(base_url)`, `::openai(base_url)`, `::vertex(base_url)`
**Builder**: `.with_auth_token(token)`, `.with_http_client(client)`

`complete()` calls `POST /proxy/{provider}/complete`, returns `Err(LlmError::RateLimited)` on 429.
`complete_streaming()` calls `POST /proxy/{provider}/stream`, wraps response in `ProxyLlmStream`.

**Wire type** (`LlmProxyResponse`) mirrors `LlmResponse`; bidirectional `From` impls keep it compatible with core types.

---

## SSE Streaming

### SSE Wire Format

```
event: llm
data: {"event_type":"text_delta","content":"..."}

event: llm
data: {"event_type":"tool_call_delta","call_id":"...","tool_name":"...","arguments_fragment":"..."}

event: llm
data: {"event_type":"completed","message":{...},"tool_calls":[...],"usage":{"input_tokens":150,"output_tokens":300}}

event: llm
data: {"event_type":"error","message":"..."}
```

Only `event: llm` entries are processed; others (e.g. ping) are silently skipped.

### ProxyLlmStream Parser

`crates/loom-server-llm-proxy/src/stream.rs`

- Maintains a `buffer: String` across TCP packets.
- Splits on `\n\n` double-newline boundary to delimit complete SSE blocks.
- Deserializes `LlmStreamEvent` and converts to `LlmEvent`.
- Invalid UTF-8 sequences are returned as `Err`.
- **Property-based tests** validate text preservation under arbitrary packet fragmentation.

---

## LlmService (Server-Side Orchestration)

**Location**: `crates/loom-server-llm-service/`

```rust
pub enum AnthropicClientWrapper {
    ApiKey(AnthropicClient),
    OAuthPool(AnthropicPool),
}
```

`LlmService` holds optional clients for each provider and exposes `complete_anthropic()`, `complete_streaming_anthropic()`, `complete_openai()`, etc.

### Default Models

| Provider | Default Model |
|---|---|
| Anthropic | `claude-3-5-sonnet-20241022` |
| OpenAI | `gpt-4o` |
| Vertex AI | `gemini-2.0-flash` |
| Z.ai | provider-specific |

### Configuration Sources (Priority Order)

1. Environment variables (`LOOM_SERVER_ANTHROPIC_API_KEY`, `LOOM_SERVER_OPENAI_API_KEY`, etc.)
2. TOML file: `~/.config/loom/llm-config.toml`
3. Runtime HTTP setup endpoints

**Key env vars**:

| Variable | Purpose |
|---|---|
| `LOOM_SERVER_ANTHROPIC_API_KEY` | Single Anthropic API key |
| `LOOM_SERVER_ANTHROPIC_OAUTH_POOL` | JSON array of OAuth accounts for pool |
| `LOOM_SERVER_ANTHROPIC_COOLDOWN_SECS` | Pool cooldown (default 7200) |
| `LOOM_SERVER_ANTHROPIC_SELECTION_STRATEGY` | `round_robin` or `first_available` |
| `LOOM_SERVER_OPENAI_API_KEY` | OpenAI key |
| `LOOM_SERVER_OPENAI_ORG_ID` | Optional OpenAI org |
| `LOOM_SERVER_VERTEX_PROJECT_ID` | GCP project |
| `LOOM_SERVER_ZAI_API_KEY` | Z.ai key |

---

## Anthropic Provider

### AnthropicClient (Single Account)

Calls `POST https://api.anthropic.com/v1/messages`.
Required headers: `Authorization: Bearer {key}`, `Anthropic-Version: 2023-06-01`.
Streaming requests add `anthropic-beta: streaming-extended-thinking-2025-08-06`.

**Error classification**:

| HTTP / message | Classification |
|---|---|
| 408, 429, 5xx | `Transient` — retry with backoff |
| "usage quota" / "rate limited" | `QuotaExceeded` — cooldown 2 h, failover |
| "invalid api key" / "auth failed" | `PermanentAuthError` — disable account |

### AnthropicPool (Multi-Account)

```rust
pub struct AnthropicPool {
    accounts: Vec<PooledAccount>,
    selection_strategy: SelectionStrategy,
    cooldown_duration: Duration,  // default: 2 hours
}
```

### Account States

| State | Description | Recovery |
|---|---|---|
| `Available` | Healthy, accepts requests | — |
| `CoolingDown` | Hit quota limit (429) | Auto-recovers when `cooldown_until` expires |
| `Disabled` | Permanent auth failure (401/403) | Manual intervention required |

### Selection Strategies

| Strategy | Behavior |
|---|---|
| `RoundRobin` | Distributes across all available accounts |
| `FirstAvailable` | Always picks lowest-index available account |

### Failover Loop

```
select_account_index()
  → check cooldown expiry → auto-upgrade CoolingDown → Available
  → pick account by strategy

on 429 → set CoolingDown + cooldown_until = now + 2h → retry with next account
on 401 → set Disabled → retry with next account
on other error → return Err immediately
all accounts exhausted → Err(NoAccountsAvailable)
```

### Background Token Refresh

Runs every 1 hour. For each account near expiry: refreshes OAuth token; on refresh failure sets account `Disabled`.

### Pool Status API

`pool.pool_status()` returns `PoolStatus { accounts: Vec<AccountHealthInfo>, healthy_count }`.
Exposed via `/health` response and `LlmService::anthropic_account_details()`.

---

## OAuth Authentication Flow (Anthropic)

```
1. GET /v1/auth/oauth/anthropic/initiate
   ← { verification_code, device_code, expires_in: 600, interval: 5 }

2. User approves in Anthropic console using verification_code

3. Poll: GET /v1/auth/oauth/anthropic/token?device_code=...  (every 5 s)
   ← "pending" | "expired" | { token, expires_at }

4. Token stored: ~/.config/loom/oauth-tokens/anthropic/{account-id}.json
   { account_id, access_token, refresh_token, expires_at }

5. loom-server loads all tokens on startup → builds AnthropicPool
   Background task refreshes tokens before expiry
```

Credential helper uses git-credential protocol style with `loom credential-helper get|store`.

---

## OpenAI Provider

Calls `POST https://api.openai.com/v1/chat/completions`.

**Headers**: `Authorization: Bearer {key}`, optional `OpenAI-Organization: {org_id}`.

**Tools** are converted from `ToolDefinition` to OpenAI's `{ type: "function", function: { name, description, parameters } }` format.
`tool_choice` is `"none"` when no tools present, `"auto"` otherwise.

**429 handling**: parses `Retry-After` header for backoff duration.

OpenAI streaming uses `data: {...}` / `data: [DONE]` SSE format (no `event:` line).

---

## Retry Strategy

Both Anthropic and OpenAI clients share:

```rust
pub struct RetryStrategy {
    pub max_retries: u32,            // default: 3
    pub initial_backoff: Duration,   // default: 100 ms
    pub max_backoff: Duration,       // default: 10 s
    pub backoff_multiplier: f64,     // default: 2.0 (exponential)
}
```

`TransientError` and `RateLimited` trigger retries. `AuthenticationFailed`, `QuotaExceeded`, `InvalidRequest`, and similar are returned immediately.

---

## Rate Limiting & Security

| Component | Function |
|---|---|
| `RateLimiter` | Per-user/per-IP: max requests/window + max tokens/window |
| `PathSanitizer` | Regex-redacts credentials from request paths in logs |
| `QueryValidator` | Checks max payload size, blocks banned patterns |
| `QueryTracer` | Per-request timeline events (start, provider_call, complete) |
| `QueryMetrics` | Prometheus counters/histograms for requests, tokens, errors, latency |

---

## Token Usage & Analytics

`Usage` is captured per-response and aggregated server-side:

```rust
pub struct UsageAnalytics {
    pub by_provider: HashMap<String, ProviderUsage>,
    pub by_user: HashMap<String, UserUsage>,
    pub by_model: HashMap<String, ModelUsage>,
}

pub struct UserUsage {
    pub daily_budget: Option<u32>,    // enforced daily token cap
    pub current_daily_usage: u32,
    // ...
}
```

Cost is calculated per-call from per-provider/model pricing tables.

---

## Prometheus Metrics (Selected)

| Metric | Labels |
|---|---|
| `llm_proxy_requests_total` | `provider`, `method` |
| `llm_proxy_request_duration_seconds` | `provider`, quantile |
| `llm_proxy_tokens_input_total` | `provider` |
| `llm_proxy_tokens_output_total` | `provider` |
| `llm_proxy_errors_total` | `provider`, `error_type` |
| `llm_anthropic_pool_account_status` | `account_id`, `status` |
| `llm_anthropic_pool_cooldown_expirations_total` | — |
| `llm_proxy_rate_limit_hits_total` | `user_id` |
| `llm_proxy_stream_events_total` | `provider`, `event_type` |
| `llm_proxy_stream_parse_errors_total` | — |

---

## Weaver Integration (Kubernetes Pods)

Weavers are K8s pods that run `loom-cli` over a WireGuard tunnel. They make identical HTTP calls to `loom-server` — no special LLM proxy path.

```
weaver pod
  └─ loom-cli --server-url https://loom.example.com --auth-token $TOKEN
       └─ POST /proxy/anthropic/complete   (same as local client)
```

Weaver lifecycle commands: `loom weaver list|create|delete|exec`.

---

## Bidirectional Streaming: ServerQuery

During an SSE stream, `loom-server` can inject a question to the client:

```
event: llm
data: {"event_type":"server_query","id":"q-1","query":"Commit these changes?","context":{...}}
```

Client responds via `POST /proxy/{provider}/query/{query_id}` with `{ "response": "yes" }`.

This is a **NEW capability** not present in ABA's current design.

---

## LLM Query Handler (Context Injection)

`LlmQueryHandler` classifies incoming request text via a `QueryDetector` trait and injects context:

| QueryType | Injected Context |
|---|---|
| `CodeGeneration` | Coding standards |
| `CodeReview` | Style guide + security checklist |
| `Documentation` | Template |
| `Debugging` | Stack trace context |
| `Generic` | None |

This is a **NEW capability** ABA does not currently implement.

---

## Key Insights for ABA

1. **Server-side proxy is worth adopting.** ABA currently holds API keys in `~/.config/ABA/config.toml` on the local machine. A proxy server would isolate credentials from agents running in CI or remote workers, matching Loom's security model.

2. **`AnthropicPool` enables throughput at scale.** ABA runs one agent loop per `cargo run`. A pool of Claude Max accounts with round-robin selection and 2-hour quota cooldown would let the Ralph loop iterate much faster before hitting rate limits.

3. **The `LlmClient` trait is ABA's natural abstraction target.** ABA already has an `LlmClient` trait (`llm.rs`) and `AnthropicClient`/`OpenAiOAuthClient`. The Loom pattern of wrapping both behind a unified `LlmRequest`/`LlmResponse` with a builder is directly applicable.

4. **SSE streaming with `LlmEvent::TextDelta` enables real-time progress.** ABA's current agent loop waits for full responses. Streaming would improve interactivity and allow partial result inspection mid-turn.

5. **`ServerQuery` (bidirectional mid-stream questions) is a novel control mechanism.** Loom's server can pause a stream and ask the client a question. For ABA's Ralph loop this could be: "This patch touches 40 files — continue?" before a hard-to-revert commit.

6. **Query-type detection + context injection is low-effort, high-value.** Detecting `CodeGeneration` vs `Debugging` and injecting appropriate system context improves response quality without changing the agent loop.

7. **Token budget enforcement per-user/per-day belongs at the proxy layer, not in each agent.** ABA should track `Usage` from every `LlmResponse` and accumulate it persistently — currently this data is discarded.

8. **`RetryStrategy` parameters (3 retries, 100ms initial, 2× backoff, 10s cap) are worth adopting verbatim.** ABA's current retry logic is ad-hoc.
