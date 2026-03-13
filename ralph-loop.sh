#!/bin/bash

# The Ralph Wiggum Continuous Execution Loop
# "I'm helping!"

set -e

WORKSPACE=$1
PROMPT_FILE=$2

if [ -z "$WORKSPACE" ] || [ -z "$PROMPT_FILE" ]; then
    echo "Usage: ./ralph-loop.sh <target-workspace> <prompt-file>"
    exit 1
fi

export RUST_LOG=info

echo "Starting the Ralph loop..."
echo "Target: $WORKSPACE"
echo "Prompt: $PROMPT_FILE"

while :; do
    echo "======================================"
    echo "[RALPH LOOP] Spawning Agent Iteration"
    echo "======================================"
    
    # We pipe the prompt text into the CLI
    cat "$PROMPT_FILE" | cargo run -- --workspace "$WORKSPACE"
    
    EXIT_CODE=$?
    
    if [ $EXIT_CODE -eq 0 ]; then
        echo "[RALPH LOOP] Agent finished iteration successfully."
        # The agent's PostToolsHook already ran tests and committed if successful.
        # We can check if tests are still passing to safely exit the infinite loop,
        # or we could keep looping if instructed to always monitor.
        
        cd "$WORKSPACE"
        if cargo test > /dev/null 2>&1; then
           echo "[RALPH LOOP] Fitness function passes! Stopping loop."
           break
        fi
        cd - > /dev/null
    else
        echo "[RALPH LOOP] Agent crashed or failed (exit code $EXIT_CODE). Restarting loop."
    fi
    
    sleep 2
done
