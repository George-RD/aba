#!/bin/bash
# ABA Ralph Wiggum Loop
# Usage: ./loop.sh [plan] [max_iterations]
# Examples:
#   ./loop.sh              # Build mode, unlimited
#   ./loop.sh 20           # Build mode, max 20 iterations
#   ./loop.sh plan         # Plan mode, unlimited
#   ./loop.sh plan 5       # Plan mode, max 5 iterations

# Don't exit on error — the agent loop handles its own errors.
# The loop must survive failed iterations.
set +e

# --- Parse arguments ---
MODE="build"
MAX_ITERATIONS=0

for arg in "$@"; do
    if [ "$arg" = "plan" ]; then
        MODE="plan"
    elif [[ "$arg" =~ ^[0-9]+$ ]]; then
        MAX_ITERATIONS=$arg
    fi
done

# --- Environment variable fallbacks (positional args take precedence) ---
if [ "$MODE" = "build" ] && [ -n "$ABA_MODE" ]; then
    MODE="$ABA_MODE"
fi
if [ "$MAX_ITERATIONS" -eq 0 ] && [ -n "$ABA_MAX_ITERATIONS" ]; then
    if [[ "$ABA_MAX_ITERATIONS" =~ ^[0-9]+$ ]]; then
        MAX_ITERATIONS="$ABA_MAX_ITERATIONS"
    else
        echo "WARNING: ABA_MAX_ITERATIONS='$ABA_MAX_ITERATIONS' is not a number. Ignoring."
    fi
fi

# --- Validate mode ---
if [ "$MODE" != "build" ] && [ "$MODE" != "plan" ]; then
    echo "ERROR: Invalid mode '$MODE'. Must be 'build' or 'plan'."
    exit 1
fi

# --- Spec path ---
SPEC_PATH="${ABA_SPEC_PATH:-golden}"
export ABA_SPEC_PATH="$SPEC_PATH"

# --- Select prompt file ---
if [ "$MODE" = "plan" ]; then
    PROMPT_FILE="PROMPT_plan.md"
else
    PROMPT_FILE="PROMPT_build.md"
fi

if [ ! -f "$PROMPT_FILE" ]; then
    echo "ERROR: $PROMPT_FILE not found in $(pwd)"
    exit 1
fi

# --- Detect VCS and branch ---
if command -v jj &> /dev/null && jj root &> /dev/null; then
    VCS="jj"
    CURRENT_BRANCH=$(jj log -r @ --no-graph -T 'bookmarks' 2>/dev/null || echo "working-copy")
else
    VCS="git"
    CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "detached")
fi

# --- Check if remote is configured ---
HAS_REMOTE=false
if [ "$VCS" = "git" ] && git remote get-url origin &>/dev/null; then
    HAS_REMOTE=true
elif [ "$VCS" = "jj" ] && jj git remote list 2>/dev/null | grep -q origin; then
    HAS_REMOTE=true
fi

# --- Detect container environment (Docker, Podman, systemd-nspawn) ---
IN_DOCKER=false
if [ -f "/.dockerenv" ] || [ -f "/run/.containerenv" ] || [ -n "${container:-}" ]; then
    IN_DOCKER=true
    HAS_REMOTE=false  # Skip git push in containers
fi

echo "======================================"
echo "  ABA Ralph Wiggum Loop"
echo "  Mode: $MODE"
echo "  Prompt: $PROMPT_FILE"
echo "  VCS: $VCS"
echo "  Branch: $CURRENT_BRANCH"
echo "  Remote: $($HAS_REMOTE && echo 'yes' || echo 'no (local only)')"
echo "  Max iterations: $([ $MAX_ITERATIONS -eq 0 ] && echo 'unlimited' || echo $MAX_ITERATIONS)"
echo "  Spec path: $SPEC_PATH"
echo "  Docker: $($IN_DOCKER && echo 'yes' || echo 'no')"
echo "======================================"

export RUST_LOG=info

# --- Proxy configuration (VPS deployment) ---
# If PROXY_BASE_URL is set, ABA routes through the local API proxy.
# The proxy injects auth headers — ABA never sees API keys.
if [ -n "$PROXY_BASE_URL" ]; then
    echo "  Proxy: $PROXY_BASE_URL"
fi

ITERATION=1

while true; do
    # Check max iterations
    if [ $MAX_ITERATIONS -gt 0 ] && [ $ITERATION -gt $MAX_ITERATIONS ]; then
        echo "Reached max iterations ($MAX_ITERATIONS). Stopping."
        break
    fi

    echo "======================== LOOP $ITERATION ========================"

    # Use the release binary if available, otherwise cargo run
    if [ -x "./target/release/aba" ]; then
        cat "$PROMPT_FILE" | ./target/release/aba
    elif command -v aba &> /dev/null; then
        cat "$PROMPT_FILE" | aba
    else
        cat "$PROMPT_FILE" | cargo run
    fi

    EXIT_CODE=$?
    if [ $EXIT_CODE -ne 0 ]; then
        echo "ABA exited with code $EXIT_CODE. Continuing to next iteration..."
    fi

    # Push to remote (only if configured)
    if $HAS_REMOTE; then
        if [ "$VCS" = "jj" ]; then
            jj git push 2>/dev/null || true
        else
            git push origin "$CURRENT_BRANCH" 2>/dev/null || git push -u origin "$CURRENT_BRANCH" 2>/dev/null || true
        fi
    fi

    ITERATION=$((ITERATION + 1))
done
