# Loom Infrastructure Reference

Source: Geoffrey Huntley's [ghuntley/loom](https://github.com/ghuntley/loom) (trunk branch).
Compiled from research conversation in `docs/research/loom-infrastructure-nix-deploy.md`.

---

## Nix Flake + cargo2nix Build System

Loom uses **cargo2nix** for reproducible, per-crate builds across 30+ Rust crates.

**Build commands:**
```bash
nix build .#loom-cli-c2n      # CLI binary
nix build .#loom-server-c2n   # HTTP API server
nix build .#<crate>-c2n       # Any individual crate
nix build .#                  # All crates via flake.nix
```

**Cargo.nix regeneration:** When `Cargo.lock` or migration files change, run `cargo2nix-update` before flake builds will succeed.

**Per-crate caching:** Each crate's dependencies are cached independently. Only crates whose dependencies change are recompiled.

**flake.nix outputs:**
- Default package (CLI or server)
- `devShell` — full dev environment (Rust toolchain, Node.js, watchexec, ripgrep)
- Docker/OCI images via `docker/` config
- NixOS modules for loom-server systemd unit

**devenv.nix / shell.nix:** `devenv` pins exact versions via `devenv.yaml` + `devenv.lock`. `shell.nix` provides `nix-shell` compatibility. `.envrc` enables automatic loading.

**Workspace build (for iteration):**
```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
make check   # format + lint + build + test (pre-commit gate)
```

Cargo is faster than Nix for incremental changes; Nix is used for production builds and CI.

---

## Rust Workspace Crate Map

| Crate | Purpose |
|---|---|
| `loom-core` | Agent state machine, conversation flow |
| `loom-server` | HTTP API server, LLM proxy, database |
| `loom-cli` | Command-line REPL |
| `loom-thread` | Conversation persistence and sync |
| `loom-tools` | Tool registry and execution |
| `loom-auth-*` | Authentication / authorization |
| `loom-llm-*` | LLM provider integrations |
| `loom-tui-*` | Terminal UI components |

---

## NixOS Auto-Update Cycle (10-Second Polling)

A systemd service (`nixos-auto-update.service`) polls for new commits every **10 seconds** and performs a fully atomic rebuild.

**Poll loop:**
1. Fetch latest commit hash from trunk
2. Compare to `/var/lib/nixos-auto-update/deployed-revision`
3. If different → `nix flake update && nixos-rebuild switch`
4. On success → write new hash to `deployed-revision`
5. On failure → previous revision stays live (automatic rollback)

**Force rebuild (recovery):**
```bash
sudo rm /var/lib/nixos-auto-update/deployed-revision
sudo systemctl restart nixos-auto-update.service
```

---

## Deployment Pipeline

```
git push trunk
  ↓
nixos-auto-update.service (polls every 10 sec)
  ↓  detects new commit
nix flake update
  ↓
cargo2nix-update (if Cargo.lock changed)
  ↓
nixos-rebuild switch  (atomic; rollback on failure)
  ↓
systemd restart loom-server.service
  ↓
Kubernetes: weaver pods pull latest OCI image
  ↓
Production live — zero downtime
```

**Troubleshooting:** `journalctl -u loom-server.service -f`

---

## CI/CD Workflows

| Workflow | Trigger | Purpose |
|---|---|---|
| Build & test | PR / push to trunk | Compile workspace, run `cargo test --workspace` |
| Lint & format | PR / push to trunk | `cargo clippy`, `cargo fmt` check |
| Binary publish | Release tag | Cross-compile and upload platform binaries to GitHub Releases |
| Agent repair loop | Deployment failure detected | `watch-nixos-auto-update.md` agent subworkflow auto-fixes Cargo.nix and redeploys |

---

## Self-Update Mechanism (`/bin/{platform}`)

`loom-server` exposes a binary distribution endpoint:

```
GET /bin/{platform}
```

**Platforms:** `linux-x86_64`, `macos-aarch64`, `macos-x86_64`, `windows-x86_64`

**Agent self-update flow:**
1. Agent calls `/bin/{platform}`
2. Compares checksum against local binary
3. If newer: download, verify, replace binary, restart
4. Entire fleet upgrades without manual intervention

---

## Database — SQLite WAL Mode

**Engine:** SQLite with WAL (Write-Ahead Logging)

**Stores:** conversation threads, user auth records, agent execution logs

**Migrations:** `crates/loom-server/migrations/NNN_description.sql`
- Sequentially numbered (`001_initial_schema.sql`, `002_...`)
- Applied on server startup
- Reversible for rollback

**WAL benefits:**
- Concurrent readers during writes
- Crash safety via write-ahead log
- Reduced fsync overhead vs. journal mode

---

## Self-Healing Agent Workflow (`watch-nixos-auto-update`)

`.agents/workflows/watch-nixos-auto-update.md` defines an autonomous repair agent:

1. Subagent monitors deployment logs
2. On failure (commonly: stale `Cargo.nix` after `Cargo.lock` change):
   - `git pull trunk`
   - Run `cargo2nix-update` to regenerate `Cargo.nix`
   - Commit and push the fix
   - Auto-deploy triggers again via 10-second poller
3. Embodies the Ralph Wiggum Loop: infrastructure self-repairs without human intervention

---

## Supporting Infrastructure

**Kubernetes weavers:**
- Namespace: `loom-weavers`
- Each weaver = ephemeral K8s pod running a persistent loom REPL process
- OCI images built reproducibly via Nix flake
- `sudo kubectl get pods -n loom-weavers`

**Networking:**
- WireGuard tunnels for all inter-node communication
- DERP relay (Designated Encrypted Relay for Packets) as fallback when direct P2P fails — enables agents behind NAT to reach weavers

**Secret management:**
- NixOS declarative secrets (likely sops-nix or similar)
- API keys (Anthropic, OpenAI) server-side only; never in client binaries
- TLS via ACME / Let's Encrypt

**Specs index** (`specs/README.md`): `architecture.md`, `state-machine.md`, `tool-system.md`, `thread-system.md`, `streaming.md`, `container-system.md`, `binary-builds-and-self-update.md`

---

## Key Insights for ABA

1. **cargo2nix is the missing link for ABA's Nix flake.** ABA's `flake.nix` currently exists but per-crate reproducibility and caching require cargo2nix integration. Without it, flake builds won't cache incrementally.

2. **The 10-second NixOS poller eliminates manual deploys entirely.** ABA can adopt the same pattern: a simple systemd service that polls a git remote and calls `nixos-rebuild switch` is the entire CD pipeline.

3. **`deployed-revision` file as deploy state.** Trivially simple deploy state — one file containing a commit hash. No external state store, no Terraform, no database. ABA should copy this pattern.

4. **The self-healing agent workflow is a Ralph loop applied to infrastructure.** `watch-nixos-auto-update.md` is not a human runbook — it is an agent prompt. The agent reads it, detects the failure mode (stale Cargo.nix), and executes the fix. ABA should define `.agents/workflows/` repair prompts for its own common failure modes.

5. **`/bin/{platform}` for self-update makes ABA a distribution point.** Once ABA serves its own binary, agents can self-upgrade. This is prerequisite for autonomous fleet management.

6. **SQLite WAL + numbered migrations is the right default for ABA.** No Postgres dependency, no migration framework — just sequentially numbered `.sql` files applied at startup.

7. **NEW: `make check` as the universal quality gate.** Loom uses a single `make check` target that enforces format + lint + build + test before any commit. ABA's `loop.sh` uses `cargo test` as its fitness function — adding `make check` (or an equivalent) raises the bar and catches format/lint regressions the Ralph loop would otherwise ignore.

8. **NEW: devenv.nix pins the entire dev environment.** ABA's `flake.nix` provides a shell but does not use devenv. Adopting devenv would give contributors a `devenv up` entry point and version-locked toolchain without requiring manual rustup setup.
