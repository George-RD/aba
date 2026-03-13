#!/bin/bash
set -e

# Clone the repo if REPO_URL is set and /workspace isn't already a git repo
if [ -n "$REPO_URL" ] && [ ! -d "/workspace/.git" ]; then
    echo "Cloning $REPO_URL..."
    git clone "$REPO_URL" /workspace
    cd /workspace
fi

# Configure git auth if GIT_TOKEN is provided (for pushing)
if [ -n "$GIT_TOKEN" ]; then
    git config --global credential.helper store
    # Extract host from remote URL
    REMOTE_URL=$(git remote get-url origin 2>/dev/null || echo "")
    if [ -n "$REMOTE_URL" ]; then
        HOST=$(echo "$REMOTE_URL" | sed -n 's|https://\([^/]*\)/.*|\1|p')
        if [ -n "$HOST" ]; then
            echo "https://x-access-token:${GIT_TOKEN}@${HOST}" > ~/.git-credentials
        fi
    fi
fi

# Build args for loop.sh
LOOP_ARGS=""
if [ "$LOOP_MODE" = "plan" ]; then
    LOOP_ARGS="plan"
fi
if [ -n "$MAX_ITERATIONS" ]; then
    LOOP_ARGS="$LOOP_ARGS $MAX_ITERATIONS"
fi

echo "Starting ABA Ralph Loop..."
echo "  Mode: ${LOOP_MODE:-build}"
echo "  Max iterations: ${MAX_ITERATIONS:-unlimited}"

exec /usr/local/bin/loop.sh $LOOP_ARGS
