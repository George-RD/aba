# Security: API Proxy & Key Management

## Overview

ABA runs as an autonomous agent loop on a VPS. It needs API keys to call LLM providers (Anthropic, OpenAI), but the agent must never have direct access to those keys. This spec defines a security architecture where API keys are held by an isolated reverse proxy, and ABA interacts with LLMs through that proxy without ever seeing the credentials.

This is a hard security boundary, not a convention. Even if ABA is compromised via prompt injection or a malicious tool output, the API keys remain inaccessible.

## Threat Model

| Threat | Mitigation |
|--------|-----------|
| ABA reads API keys from config/env | Keys are only in the proxy's config, owned by a different system user |
| ABA reads proxy process memory via /proc | Linux user isolation: aba user cannot read aba-proxy's /proc entries |
| ABA exfiltrates keys through conversation logs | Secret redaction scans all persisted text |
| Prompt injection tricks ABA into leaking keys | ABA never has keys to leak; proxy injects them server-side |
| Stolen VPS root access | SOPS + age encryption at rest; keys only decrypted into proxy config |
| Key rotation requires ABA cooperation | Human-in-the-loop: key changes require SSH approval |

## API Proxy Pattern

### Architecture

```
ABA (user: aba)                     nginx (user: aba-proxy)           External API
     |                                    |                                |
     |-- POST localhost:8080/anthropic/ ->|                                |
     |                                    |-- + x-api-key header --------->|
     |                                    |<- response --------------------|
     |<- response (no key in it) ---------|                                |
     |                                    |                                |
     |-- POST localhost:8080/openai/ ---->|                                |
     |                                    |-- + Authorization: Bearer ---->|
     |                                    |<- response --------------------|
     |<- response (no key in it) ---------|                                |
```

### Proxy Behavior

The nginx reverse proxy runs as a systemd service under the `aba-proxy` system user. It:

1. **Listens on `127.0.0.1:8080` only** -- not exposed to the network, not even via Tailscale.
2. **Routes by path prefix:**
   - `/anthropic/*` -> `https://api.anthropic.com/*` (strips the `/anthropic` prefix)
   - `/openai/*` -> `https://api.openai.com/*` (strips the `/openai` prefix)
3. **Injects authentication headers** per provider:
   - Anthropic: `x-api-key: <key>`
   - OpenAI: `Authorization: Bearer <key>`
4. **Strips any auth headers** ABA might send (prevents ABA from learning the header format).
5. **Logs requests without auth headers** -- method, path, status code, latency, but never the Authorization or x-api-key values.
6. **Enforces rate limits** (future) per provider to prevent runaway loops from burning credit.

### User Isolation

The security model relies on Linux user separation:

- **`aba` user**: normal user, runs the agent. Cannot read files owned by `aba-proxy`. Cannot read `/proc/<nginx-pid>/environ` or `/proc/<nginx-pid>/maps` because those are owned by `aba-proxy`/root.
- **`aba-proxy` user**: system user (no login shell, no SSH access). Owns the nginx config containing decrypted keys. nginx worker processes run as this user.
- **`root`**: deploys config, decrypts SOPS secrets into proxy config. Human access only.

File permissions:
```
/etc/nginx/api-keys.conf       root:aba-proxy  0640  # decrypted keys, included by nginx
/etc/nginx/nginx.conf           root:root       0644  # main config (no secrets)
/etc/nginx/conf.d/api-proxy.conf root:root      0644  # proxy routing (no secrets)
```

The key file (`api-keys.conf`) is readable by `aba-proxy` (for nginx workers) but not by `aba`.

### ABA Client Configuration

ABA's `config.toml` points at the proxy instead of the real API:

```toml
[providers.anthropic]
base_url = "http://127.0.0.1:8080/anthropic"
# No api_key field -- the proxy handles it

[providers.openai]
base_url = "http://127.0.0.1:8080/openai"
# No api_key field -- the proxy handles it
```

The `LlmClient` implementations in `llm.rs` must support configurable base URLs. When a base URL is set and no API key is configured, the client sends requests without auth headers (the proxy adds them).

## Human-in-the-Loop Key Management

### Principles

1. **ABA can never add, read, or modify API keys directly.**
2. **All key changes require human approval** via SSH session or CLI tool.
3. **Key values are passed out-of-band** -- never through the agent conversation.

### Key Lifecycle

#### Adding a New Key

```
1. ABA detects it needs a new provider (e.g., a new model endpoint)
2. ABA writes a request file:
   /var/lib/aba/key-requests/2026-03-16T14:30:00-add-gemini.json
   {
     "action": "add",
     "provider": "gemini",
     "reason": "Need Gemini 2.0 for code generation benchmark",
     "requested_at": "2026-03-16T14:30:00Z"
   }
   Note: NO key value in the request. ABA doesn't have it.
3. Human receives notification (email, Slack webhook, or checks manually)
4. Human reviews the request via SSH:
   $ aba-keys review
   Pending: add gemini key (requested 2026-03-16T14:30:00Z)
   Reason: Need Gemini 2.0 for code generation benchmark
   Approve? [y/N]: y
   Enter API key: <pasted from provider dashboard, not echoed>
5. The tool:
   a. Encrypts the key into secrets/api-keys.yaml via SOPS
   b. Updates nginx config
   c. Reloads nginx (systemctl reload nginx)
   d. Writes approval record to /var/lib/aba/key-requests/
6. ABA sees the request was approved and can now use the new provider endpoint
```

#### Rotating a Key

Same flow as adding, but `action: "rotate"` and the old key is replaced. The human obtains the new key from the provider dashboard and enters it via the CLI tool.

#### Revoking a Key

```
1. Human decides to revoke (e.g., suspected compromise)
2. $ aba-keys revoke anthropic
3. Tool removes key from nginx config, reloads nginx
4. ABA's requests to /anthropic/* start returning 401
5. ABA detects the failure and can request re-addition through the normal flow
```

### Request File Format

Requests live in `/var/lib/aba/key-requests/` and are writable by the `aba` user:

```json
{
  "id": "uuid-v4",
  "action": "add|rotate|revoke",
  "provider": "anthropic|openai|gemini|...",
  "reason": "free-text explanation from the agent",
  "requested_at": "ISO-8601",
  "status": "pending|approved|denied",
  "reviewed_at": null,
  "reviewed_by": null
}
```

The `aba` user can write new requests and read status. The `aba` user cannot modify existing requests after creation (append-only via directory permissions or a simple daemon).

## Secret Redaction

### What Gets Redacted

All text persisted by ABA (conversation logs, tool output, thread files) must be scanned for patterns that look like API keys:

| Provider | Pattern | Example |
|----------|---------|---------|
| Anthropic | `sk-ant-api03-[A-Za-z0-9_-]{90,}` | `sk-ant-api03-abc123...` |
| OpenAI | `sk-[A-Za-z0-9]{48,}` | `sk-abc123...` |
| Generic | Any Base64 string > 40 chars adjacent to "key", "token", "secret", "authorization" | Various |

### Redaction Strategy

1. **At write time**: Before any conversation turn is written to disk, scan the text content against known key patterns. Replace matches with `[REDACTED:<provider>-key]`.
2. **In the proxy**: nginx access logs never include `x-api-key` or `Authorization` header values. The `proxy_hide_header` and custom log format handle this.
3. **Defense in depth**: Even though ABA should never see a key, redaction catches prompt injection scenarios where an attacker tries to get ABA to echo back a key it found through some unexpected channel.

### Implementation

A Rust function in the agent:

```rust
fn redact_secrets(text: &str) -> String {
    // Apply regex patterns for known key formats
    // Return sanitized text
}
```

Called in `agent.rs` before appending any message to the conversation log or writing thread files.

## SOPS + Age Integration

### Key Architecture

Two age keypairs:

1. **Developer key** -- on the engineer's local machine (`~/.config/sops/age/keys.txt`). Used to encrypt/decrypt secrets during development and initial setup.
2. **Server key** -- on the NixOS VPS (`/var/lib/sops-nix/keys.txt`). Generated during server provisioning. Used to decrypt secrets at deploy time.

Both public keys are listed in `.sops.yaml` so either can decrypt.

### Encrypted File: `secrets/api-keys.yaml`

```yaml
# Encrypted with SOPS -- edit with: sops secrets/api-keys.yaml
anthropic_api_key: ENC[AES256_GCM,data:...,type:str]
openai_api_key: ENC[AES256_GCM,data:...,type:str]
```

### Decryption Flow at Deploy Time

```
1. NixOS rebuild triggered (manual or via CI)
2. sops-nix module decrypts secrets/api-keys.yaml using the server's age key
3. Decrypted values written to /run/secrets/api-keys (tmpfs, mode 0400, owner root)
4. A systemd ExecStartPre script reads decrypted values and generates
   /etc/nginx/api-keys.conf with the real keys
5. nginx starts/reloads, includes api-keys.conf
6. /run/secrets/api-keys is only readable by root
7. /etc/nginx/api-keys.conf is only readable by root and aba-proxy group
```

The decrypted keys exist in exactly two places at runtime:
- `/run/secrets/api-keys` (tmpfs, root-only) -- the sops-nix output
- `/etc/nginx/api-keys.conf` (root:aba-proxy 0640) -- the nginx include
- nginx worker memory (aba-proxy user)

The `aba` user cannot access any of these.

### .sops.yaml Configuration

```yaml
keys:
  - &dev age1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  - &server age1yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy
creation_rules:
  - path_regex: secrets/.*\.(yaml|json|env)$
    key_groups:
      - age:
        - *dev
        - *server
```

## Future: Self-Configuration

### Goal

ABA should eventually be able to onboard new API providers autonomously -- but never handle raw keys.

### Flow

```
1. ABA decides it needs access to a new provider (e.g., Google Gemini)
2. ABA creates a key request (see Key Lifecycle above)
3. ABA updates its own code:
   a. Adds a new LlmClient variant for the provider
   b. Adds a new proxy path in the NixOS config
   c. Commits the code changes (these go through the normal Ralph loop)
4. Human reviews:
   a. The code PR (normal code review)
   b. The key request (via aba-keys CLI)
5. Human provides the API key out-of-band
6. Deploy: code + new key both land on the server
7. ABA can now use the new provider
```

The key insight is that ABA can modify its own code and configuration to support a new provider, but it can never supply the API key. The key is always provided by a human through a separate channel.

### Self-Configuration Boundaries

| ABA Can | ABA Cannot |
|---------|-----------|
| Add proxy route configs in NixOS configuration | Read or write API keys |
| Implement new LlmClient variants | Access the proxy's key config file |
| Request key additions/rotations | Approve its own requests |
| Update its own base_url config | Modify nginx's running config directly |
| Monitor provider health via the proxy | Bypass the proxy to call APIs directly |

## Relationship to Other Specs

- **agent-core.md**: The `LlmClient` trait needs configurable `base_url` to support the proxy pattern. The PostToolsHook's commit/revert cycle is unaffected.
- **source-control.md**: No impact. VCS operations are local and don't involve API keys.
- **self-bootstrapping.md**: This security architecture should be implemented early (Tier 1.5) because it's a prerequisite for running ABA on a remote VPS. The agent needs API access to build itself, and that access must be secure from day one.
