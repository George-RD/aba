#!/usr/bin/env bash
# ABA Milestone 0 REPL -- conversational interface to ABA via the API proxy.
#
# Dependencies: curl, jq (both available on NixOS)
# Usage: ./scripts/repl.sh
#        Or via alias: aba
#
# The REPL sends messages to the LLM through the proxy at $PROXY_BASE_URL
# (default: http://127.0.0.1:8080/openai) using the OpenAI chat completions
# API format. Responses are streamed token-by-token.

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
PROXY_BASE_URL="${PROXY_BASE_URL:-http://127.0.0.1:8080/openai}"
ABA_MODEL="${ABA_MODEL:-gpt-4o}"
MAX_HISTORY=20  # Keep last N messages to avoid context overflow

# Resolve ABA_DIR: directory containing this script's parent
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ABA_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Tempfile for conversation history (JSON array)
HISTORY_FILE="$(mktemp /tmp/aba-repl-history.XXXXXX.json)"

# Tempfile for streaming response assembly
RESPONSE_FILE="$(mktemp /tmp/aba-repl-response.XXXXXX.txt)"

# Track whether we are currently streaming (for Ctrl+C handling)
STREAMING_PID=""

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------
cleanup() {
    [[ -n "$STREAMING_PID" ]] && kill "$STREAMING_PID" 2>/dev/null || true
    rm -f "$HISTORY_FILE" "$RESPONSE_FILE"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Ctrl+C: cancel current request, don't exit the REPL
# ---------------------------------------------------------------------------
handle_sigint() {
    if [[ -n "$STREAMING_PID" ]]; then
        kill "$STREAMING_PID" 2>/dev/null || true
        STREAMING_PID=""
        echo ""
        echo "[request cancelled]"
    fi
    # Do not exit -- return to the prompt
}
trap handle_sigint INT

# ---------------------------------------------------------------------------
# Build system prompt
# ---------------------------------------------------------------------------
build_system_prompt() {
    local system_prompt=""

    # Load BOOTSTRAP.md if available
    local bootstrap_file=""
    if [[ -f "$ABA_DIR/BOOTSTRAP.md" ]]; then
        bootstrap_file="$ABA_DIR/BOOTSTRAP.md"
    elif [[ -f "$SCRIPT_DIR/BOOTSTRAP.md" ]]; then
        bootstrap_file="$SCRIPT_DIR/BOOTSTRAP.md"
    fi

    if [[ -n "$bootstrap_file" ]]; then
        system_prompt="$(cat "$bootstrap_file")"
        system_prompt="${system_prompt}

---

"
    fi

    # Detect first-run (no config file or proxy not reachable)
    local first_run_note=""
    if [[ ! -f "$HOME/.config/ABA/config.toml" ]]; then
        first_run_note="NOTE: This appears to be a first run -- no config file found at ~/.config/ABA/config.toml. Help the user get set up. The proxy should handle auth, so the user mainly needs to verify the proxy is running and test a simple request.

"
    fi

    system_prompt="${system_prompt}${first_run_note}You are ABA, a self-improving coding agent. You are running as a conversational REPL on the user's VPS.

Current date: $(date '+%Y-%m-%d %H:%M:%S %Z')
Working directory: $(pwd)
ABA directory: ${ABA_DIR}
Model: ${ABA_MODEL}
Proxy: ${PROXY_BASE_URL}

Available commands the user can run:
  aba           -- this conversational REPL
  aba-loop      -- start the Ralph loop (build mode, unlimited iterations)
  aba-build     -- start the Ralph loop (build mode)
  aba-plan      -- start the Ralph loop (plan mode, generates IMPLEMENTATION_PLAN.md)
  aba-status    -- check proxy health

You should help the user:
- Understand what ABA is and how to use it
- Bootstrap their environment if this is a first run
- Run planning and build loops
- Debug issues with the proxy, auth, or agent
- Discuss the codebase, specs, and implementation plan

Be concise and practical. You are a tool, not a tutor. Answer questions directly."

    printf '%s' "$system_prompt"
}

# ---------------------------------------------------------------------------
# Initialize conversation history with system prompt
# ---------------------------------------------------------------------------
init_history() {
    local system_prompt
    system_prompt="$(build_system_prompt)"

    # Escape for JSON: handle newlines, quotes, backslashes, tabs
    local escaped
    escaped="$(printf '%s' "$system_prompt" | jq -Rs .)"

    echo "[{\"role\": \"system\", \"content\": ${escaped}}]" > "$HISTORY_FILE"
}

# ---------------------------------------------------------------------------
# Append a message to history
# ---------------------------------------------------------------------------
append_message() {
    local role="$1"
    local content="$2"

    local escaped
    escaped="$(printf '%s' "$content" | jq -Rs .)"

    local updated
    updated="$(jq --arg role "$role" --argjson content "$escaped" \
        '. + [{"role": $role, "content": $content}]' "$HISTORY_FILE")"

    echo "$updated" > "$HISTORY_FILE"
}

# ---------------------------------------------------------------------------
# Trim history to keep it within MAX_HISTORY messages (plus system prompt)
# ---------------------------------------------------------------------------
trim_history() {
    local count
    count="$(jq 'length' "$HISTORY_FILE")"

    # +1 for the system message
    local max_total=$(( MAX_HISTORY + 1 ))

    if (( count > max_total )); then
        # Keep the system message (index 0) and the last MAX_HISTORY messages
        local trim_from=$(( count - MAX_HISTORY ))
        local trimmed
        trimmed="$(jq --argjson from "$trim_from" \
            '[.[0]] + .[$from:]' "$HISTORY_FILE")"
        echo "$trimmed" > "$HISTORY_FILE"
    fi
}

# ---------------------------------------------------------------------------
# Send request to LLM and stream the response
# ---------------------------------------------------------------------------
send_and_stream() {
    local request_body
    request_body="$(jq -n \
        --argjson messages "$(cat "$HISTORY_FILE")" \
        --arg model "$ABA_MODEL" \
        '{
            "model": $model,
            "messages": $messages,
            "stream": true
        }')"

    # Clear response file
    > "$RESPONSE_FILE"

    # Use curl to stream SSE and parse deltas in real time.
    # We run curl in the background so Ctrl+C can cancel it.
    # The while-read loop parses SSE data lines and extracts content deltas.
    (
        curl -sS --no-buffer \
            -X POST "${PROXY_BASE_URL}/v1/chat/completions" \
            -H "Content-Type: application/json" \
            -d "$request_body" \
            2>/dev/null \
        | while IFS= read -r line; do
            # SSE lines look like: data: {"id":"...","choices":[{"delta":{"content":"token"}}]}
            # The stream ends with: data: [DONE]

            # Strip carriage return if present
            line="${line%$'\r'}"

            # Skip empty lines and non-data lines
            if [[ ! "$line" =~ ^data:\ (.+)$ ]]; then
                continue
            fi

            local data="${BASH_REMATCH[1]}"

            # End of stream
            if [[ "$data" == "[DONE]" ]]; then
                break
            fi

            # Extract the content delta
            local delta
            delta="$(printf '%s' "$data" | jq -r '.choices[0].delta.content // empty' 2>/dev/null)" || continue

            if [[ -n "$delta" ]]; then
                printf '%s' "$delta"
                printf '%s' "$delta" >> "$RESPONSE_FILE"
            fi
        done
    ) &
    STREAMING_PID=$!

    # Wait for the streaming process to finish
    wait "$STREAMING_PID" 2>/dev/null || true
    STREAMING_PID=""

    # Print a trailing newline after the streamed response
    echo ""

    # Read the full response and add it to history
    local full_response
    full_response="$(cat "$RESPONSE_FILE")"

    if [[ -n "$full_response" ]]; then
        append_message "assistant" "$full_response"
    fi
}

# ---------------------------------------------------------------------------
# Check proxy health
# ---------------------------------------------------------------------------
check_proxy() {
    local health_url="${PROXY_BASE_URL%/openai*}/health"
    # Also try with the base URL pattern
    if [[ "$PROXY_BASE_URL" == *"/openai"* ]]; then
        health_url="${PROXY_BASE_URL%/openai*}/health"
    elif [[ "$PROXY_BASE_URL" == *"/anthropic"* ]]; then
        health_url="${PROXY_BASE_URL%/anthropic*}/health"
    else
        health_url="${PROXY_BASE_URL}/health"
    fi

    local response
    if response="$(curl -sf --max-time 5 "$health_url" 2>/dev/null)"; then
        return 0
    else
        return 1
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
    echo "ABA Conversational REPL"
    echo "Model: ${ABA_MODEL} | Proxy: ${PROXY_BASE_URL}"
    echo "Type 'exit' or 'quit' to leave. Ctrl+C cancels the current request."
    echo ""

    # Check proxy health
    if ! check_proxy; then
        echo "WARNING: Proxy at ${PROXY_BASE_URL} is not responding."
        echo "The REPL will start, but requests will fail until the proxy is up."
        echo ""
        echo "To check proxy status:  curl -s http://127.0.0.1:8080/health"
        echo "To start without proxy: export PROXY_BASE_URL=https://api.openai.com"
        echo ""
    fi

    # Initialize conversation
    init_history

    # REPL loop
    while true; do
        # Reset the INT trap for the prompt (in case it was triggered during streaming)
        trap handle_sigint INT

        # Read user input
        printf 'aba> '
        if ! IFS= read -r user_input; then
            # Ctrl+D (EOF)
            echo ""
            echo "Goodbye."
            break
        fi

        # Skip empty input
        if [[ -z "${user_input// /}" ]]; then
            continue
        fi

        # Exit commands
        if [[ "$user_input" == "exit" || "$user_input" == "quit" ]]; then
            echo "Goodbye."
            break
        fi

        # Add user message to history
        append_message "user" "$user_input"

        # Trim history if needed
        trim_history

        # Send to LLM and stream response
        echo ""
        send_and_stream
        echo ""
    done
}

main "$@"
