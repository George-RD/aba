# Multi-stage build: Nix builds the binary, minimal runtime image runs it.
# Coolify: set build pack to "Dockerfile".

# Stage 1: Build with Nix
FROM nixos/nix:latest AS builder

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

WORKDIR /build
COPY . .

# Build the ABA binary via the flake
RUN nix build .#default --no-link
RUN cp $(nix path-info .#default)/bin/aba /aba

# Stage 2: Minimal runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

# Git config for auto-commits
RUN git config --global user.name "ABA Agent" && \
    git config --global user.email "aba@agent.local"

COPY --from=builder /aba /usr/local/bin/aba
COPY loop.sh /usr/local/bin/loop.sh
COPY PROMPT_plan.md /workspace/PROMPT_plan.md
COPY PROMPT_build.md /workspace/PROMPT_build.md

WORKDIR /workspace

# Environment variables (set these in Coolify):
# ANTHROPIC_API_KEY or OPENAI_API_KEY - LLM auth
# REPO_URL - git repo to clone and work on
# LOOP_MODE - "plan" or "build" (default: build)
# MAX_ITERATIONS - max loop iterations (default: unlimited)

COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

ENTRYPOINT ["docker-entrypoint.sh"]
