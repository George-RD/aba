#!/bin/bash
# ABA Ralph Wiggum Loop
# Usage: ./loop.sh [plan] [max_iterations]
# Examples:
#   ./loop.sh              # Build mode, unlimited
#   ./loop.sh 20           # Build mode, max 20 iterations
#   ./loop.sh plan         # Plan mode, unlimited
#   ./loop.sh plan 5       # Plan mode, max 5 iterations

set -e

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

# --- Detect current branch ---
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)

echo "======================================"
echo "  ABA Ralph Wiggum Loop"
echo "  Mode: $MODE"
echo "  Prompt: $PROMPT_FILE"
echo "  Branch: $CURRENT_BRANCH"
echo "  Max iterations: $([ $MAX_ITERATIONS -eq 0 ] && echo 'unlimited' || echo $MAX_ITERATIONS)"
echo "======================================"

export RUST_LOG=info
ITERATION=1

while true; do
    # Check max iterations
    if [ $MAX_ITERATIONS -gt 0 ] && [ $ITERATION -gt $MAX_ITERATIONS ]; then
        echo "Reached max iterations ($MAX_ITERATIONS). Stopping."
        break
    fi

    echo "======================== LOOP $ITERATION ========================"

    cat "$PROMPT_FILE" | cargo run

    # Push to remote
    git push origin "$CURRENT_BRANCH" 2>/dev/null || git push -u origin "$CURRENT_BRANCH"

    ITERATION=$((ITERATION + 1))
done
