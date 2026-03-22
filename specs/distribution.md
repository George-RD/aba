# Distribution: Template Repo, Modular Specs, One-Click Deploy

## Overview

This repo defines the **base ABA** -- the canonical agent binary, core specs, and deployment infrastructure. Each deployed ABA instance pulls from this base, overlays custom specs, and boots into a self-improving loop. Distribution happens through two tiers: a Docker-based template for platform marketplaces (Railway, etc.) and a NixOS flake for production deployments.

The goal: anyone can go from zero to a running, self-improving ABA in under 5 minutes.

## Architecture

```text
github.com/org/aba  (this repo -- the base)
        |
        ├── Tier 1: Docker Template (Railway, Fly.io, any Docker host)
        │     └── nixos/nix base image → Rust toolchain → ABA binary → specs → loop.sh
        │
        └── Tier 2: NixOS Flake (Garnix, Hetzner + nixos-anywhere)
              └── Full NixOS config → nginx proxy → user isolation → SOPS secrets
```

### Tier 1: Docker Template (Easy Mode)

For platform marketplaces and quick starts. Trades the NixOS security model for one-click deployment.

**Image base:** `nixos/nix` -- the Nix package manager in a minimal container. Not NixOS-the-OS, but gives us reproducible builds and the full nixpkgs ecosystem.

**What's inside:**
- Nix (for reproducible builds)
- Rust toolchain (ABA compiles and tests itself)
- Git (VCS for commits/reverts in the Ralph loop)
- ABA binary (built from the flake)
- Specs directory (modular, user-customisable)
- loop.sh, PROMPT_plan.md, PROMPT_build.md

**What's NOT inside (handled by the platform):**
- API key management → platform env vars / secrets
- User isolation → trust the platform sandbox
- SSH access → platform terminal or not needed for autonomous loops
- systemd services → single-process container, loop.sh is the entrypoint

**Secret handling:** The LLM client (`llm.rs`) must support receiving API keys via environment variables (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) with no proxy. This already partially exists in `config.rs` but must work cleanly as the primary auth path in Docker mode, not just a fallback.

### Tier 2: NixOS Flake (Production Mode)

For users who want the full security model. This is the existing `nixos/configuration.nix` path, enhanced for template distribution.

**Deployment targets:**
- **Garnix** -- push flake to GitHub, Garnix provisions a NixOS server. Closest to one-click.
- **Hetzner + nixos-anywhere** -- one command to install NixOS on a VPS. Best value (~4 EUR/month).
- **Any NixOS host** -- deploy-rs or colmena for ongoing management.

**What's inside:** Everything from Tier 1 plus nginx proxy, user isolation, SOPS/age secrets, systemd services, SSH hardening, Tailscale.

## Changes Required

### 1. Dockerfile

Create `Dockerfile` at repo root.

```dockerfile
FROM nixos/nix AS builder

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

COPY . /build
WORKDIR /build
RUN nix build .#aba --no-link
RUN nix-store --export $(nix-store -qR $(nix build .#aba --print-out-paths)) > /aba-closure

FROM nixos/nix

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

# Import the pre-built ABA closure
COPY --from=builder /aba-closure /aba-closure
RUN nix-store --import < /aba-closure && rm /aba-closure

# Runtime dependencies ABA needs for self-improvement
RUN nix profile install \
  nixpkgs#git \
  nixpkgs#rustup \
  nixpkgs#gcc \
  nixpkgs#pkg-config \
  nixpkgs#openssl \
  nixpkgs#openssl.dev

# Set up Rust toolchain
RUN rustup default stable

# Set up workspace
RUN mkdir -p /workspace
WORKDIR /workspace

# Copy project files (specs, prompts, loop script)
COPY specs/ ./specs/
COPY PROMPT_plan.md PROMPT_build.md loop.sh ./
COPY src/ ./src/
COPY Cargo.toml Cargo.lock ./
COPY flake.nix flake.lock ./
COPY .claude/ ./.claude/
RUN chmod +x loop.sh

# Persistent volume mount point for repo state, cargo cache, etc.
VOLUME /workspace

# Default: start the Ralph build loop
CMD ["./loop.sh", "build"]
```

**Notes:**
- Multi-stage build: first stage builds via Nix flake, second stage is the runtime.
- ABA needs the full Rust toolchain at runtime because it compiles itself.
- The VOLUME at /workspace persists repo state across container restarts.
- CMD is the default but can be overridden (e.g., `./loop.sh plan`, or a future `aba` REPL command).

### 2. Railway Template Files

Create `railway.toml`:

```toml
[build]
builder = "DOCKERFILE"
dockerfilePath = "Dockerfile"

[deploy]
startCommand = "./loop.sh build"
healthcheckPath = "/"
restartPolicyType = "ON_FAILURE"
restartPolicyMaxRetries = 3

[[volumes]]
mount = "/workspace"
name = "aba-workspace"
```

Create `railway.json` (template metadata for the Railway marketplace):

```json
{
  "$schema": "https://railway.app/railway.schema.json",
  "build": {
    "builder": "DOCKERFILE",
    "dockerfilePath": "Dockerfile"
  },
  "deploy": {
    "startCommand": "./loop.sh build",
    "restartPolicyType": "ON_FAILURE"
  }
}
```

**Required Railway env vars (set by user at deploy time):**
- `ANTHROPIC_API_KEY` or `OPENAI_API_KEY` -- at least one LLM provider key
- `ABA_MODE` (optional) -- `build` (default) or `plan`
- `ABA_MAX_ITERATIONS` (optional) -- loop iteration limit

**Revenue:** Railway template marketplace gives 25% of user spend on the template.

### 3. Modular Specs System

Restructure `specs/` to support user customisation and a golden path default.

#### Directory Structure

```text
specs/
  _manifest.toml          # Declares available spec modules and the golden path
  agent-core.md           # Core agent loop (always included)
  source-control.md       # VCS abstraction
  security.md             # API proxy + key management
  self-bootstrapping.md   # Milestone roadmap
  conversational-layer.md # REPL + dialogue
  observability.md        # Logging + cost tracking
  distribution.md         # This spec
```

#### Manifest File

`specs/_manifest.toml`:

```toml
# Spec module manifest -- controls which specs the planning agent reads
# and what order it prioritises them in.

[meta]
version = "0.1.0"
description = "ABA spec manifest -- defines available modules and priority paths"

# Core specs: always loaded, not optional
[[core]]
file = "agent-core.md"
description = "Multi-turn agent loop, tool system, LLM abstraction"

[[core]]
file = "source-control.md"
description = "VCS trait, git/jj backends"

# Optional modules: loaded based on the active path
[[modules]]
file = "security.md"
description = "API proxy, key isolation, SOPS/age"
tags = ["production", "vps"]

[[modules]]
file = "conversational-layer.md"
description = "REPL, thread persistence, dialogue"
tags = ["interactive", "repl"]

[[modules]]
file = "observability.md"
description = "Structured logging, cost tracking, secret redaction"
tags = ["production", "observability"]

[[modules]]
file = "distribution.md"
description = "Docker template, Railway, modular specs"
tags = ["distribution", "deployment"]

# Paths: named priority orderings for different use cases
[paths.golden]
description = "Default path -- self-improving agent that can build itself"
priority = [
  "self-bootstrapping.md",
  "agent-core.md",
  "source-control.md",
  "observability.md",
  "conversational-layer.md",
  "security.md",
  "distribution.md",
]

[paths.minimal]
description = "Minimum viable agent -- just the loop"
priority = [
  "agent-core.md",
  "source-control.md",
  "self-bootstrapping.md",
]

[paths.interactive]
description = "Conversational-first -- REPL before loops"
priority = [
  "conversational-layer.md",
  "agent-core.md",
  "source-control.md",
  "self-bootstrapping.md",
]
```

#### How the Planning Agent Uses It

Update `PROMPT_plan.md` to reference the manifest:

> Phase 0a: Read `specs/_manifest.toml` to understand available modules and the active path. Default to the `golden` path unless a different path is specified in the environment (`ABA_SPEC_PATH=minimal`).

The build agent reads specs in priority order, so the most important gaps get addressed first regardless of which specs exist.

#### User Customisation

Users fork the repo and either:
1. Edit `_manifest.toml` to reorder priorities or remove modules
2. Add their own spec files and reference them in the manifest
3. Set `ABA_SPEC_PATH=<path-name>` to switch paths

This means someone who only cares about the REPL can set `ABA_SPEC_PATH=interactive` and ABA will prioritise conversational features over observability or security.

### 4. Flake Template Outputs

Extend `flake.nix` to expose templates that users can instantiate:

```nix
# In the flake outputs:
templates = {
  default = {
    path = ./templates/default;
    description = "ABA with golden path specs -- self-improving coding agent";
  };
  minimal = {
    path = ./templates/minimal;
    description = "Minimal ABA -- just the agent loop and VCS";
  };
  railway = {
    path = ./templates/railway;
    description = "ABA Railway template -- one-click deploy to Railway marketplace";
  };
};
```

Each template directory contains:
- `flake.nix` that references the base ABA flake as an input
- `specs/` with the appropriate subset and manifest
- Platform-specific files (Dockerfile, railway.toml for the railway template)
- A README with setup instructions

Usage: `nix flake init -t github:org/aba#railway`

### 5. NixOS System Output in Flake

The current `flake.nix` only outputs packages and devShells. Add a NixOS system configuration output for Garnix/deploy-rs:

```nix
# Add to flake outputs (outside eachDefaultSystem):
nixosConfigurations.aba-vps = nixpkgs.lib.nixosSystem {
  system = "x86_64-linux";
  modules = [
    ./nixos/configuration.nix
    # Override with the locally-built ABA binary
    ({ ... }: {
      environment.systemPackages = [ self.packages.x86_64-linux.aba ];
    })
  ];
};
```

This lets Garnix build and deploy the full NixOS config from a `git push`, and lets users run `deploy-rs .#aba-vps` for manual deploys.

### 6. Environment-Aware Auth in config.rs / llm.rs

The auth flow must cleanly support three modes:

| Mode | How keys arrive | When used |
|------|----------------|-----------|
| **Proxy** | No keys in ABA; proxy injects headers | NixOS VPS (Tier 2) |
| **Env vars** | `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` from environment | Docker / Railway / Fly.io (Tier 1) |
| **Config file** | Interactive setup writes `~/.config/ABA/config.toml` | Local dev |

Detection order:
1. If `PROXY_BASE_URL` is set → proxy mode (no keys needed)
2. If `ANTHROPIC_API_KEY` or `OPENAI_API_KEY` env var is set → env var mode
3. If config file exists with keys → config file mode
4. Otherwise → interactive setup (existing flow)

This must work without any user configuration in Docker. A container with `OPENAI_API_KEY` set should Just Work.

### 7. loop.sh Updates

Make `loop.sh` platform-aware:

- Read `ABA_MODE` env var as alternative to positional arg (`./loop.sh build` vs `ABA_MODE=build ./loop.sh`)
- Read `ABA_MAX_ITERATIONS` env var as alternative to positional arg
- Read `ABA_SPEC_PATH` and pass to the agent as context
- Detect Docker environment (check for `/.dockerenv` or `container` in `/proc/1/cgroup`) and skip git push by default
- Log to stdout (not just stderr) so Railway/Docker logging works

### 8. .dockerignore

Create `.dockerignore` to keep the image lean:

```text
target/
dummy-project/
docs/
.git/
*.md
!PROMPT_plan.md
!PROMPT_build.md
!CLAUDE.md
!specs/*.md
```

## Implementation Priority

Ordered by what unblocks the most value:

1. **Environment-aware auth** (config.rs/llm.rs) -- prerequisite for everything Docker
2. **Dockerfile** -- the portable foundation
3. **loop.sh updates** -- platform-aware, env var driven
4. **.dockerignore** -- trivial but needed for clean builds
5. **Spec manifest** (`_manifest.toml`) -- enables modular paths
6. **Railway template files** (railway.toml, railway.json) -- unlocks marketplace revenue
7. **Flake template outputs** -- enables `nix flake init` distribution
8. **NixOS system output in flake** -- enables Garnix one-click
9. **PROMPT_plan.md update** -- teach the planning agent about the manifest

## Relationship to Other Specs

- **agent-core.md**: Unchanged. The agent loop works identically in Docker and NixOS.
- **security.md**: Tier 2 only. Docker deployments use platform-native secrets. The proxy architecture is a Tier 2 upgrade, not a prerequisite.
- **self-bootstrapping.md**: The milestone roadmap applies to both tiers. Docker users start at M1 (agent loop works); NixOS users get the full M0-M10 path.
- **source-control.md**: Git works in Docker. JJ is a Tier 2 / future concern.
- **conversational-layer.md**: The REPL works in both tiers. Docker users access it via platform terminal or by overriding CMD.
- **observability.md**: Logging to stdout works in Docker (platform captures it). Structured logging to files is a Tier 2 enhancement.

## Success Criteria

- [ ] `docker build -t aba .` succeeds and produces a working image
- [ ] `docker run -e OPENAI_API_KEY=sk-... aba` starts a Ralph build loop
- [ ] `docker run -e OPENAI_API_KEY=sk-... aba ./loop.sh plan` generates an implementation plan
- [ ] Railway template deploys in one click with only an API key env var
- [ ] `nix flake init -t github:org/aba` creates a working project scaffold
- [ ] `nix build .#nixosConfigurations.aba-vps.config.system.build.toplevel` builds the NixOS system
- [ ] Spec manifest correctly controls which specs the planning agent reads
- [ ] ABA_SPEC_PATH=minimal produces a different plan than the golden path
