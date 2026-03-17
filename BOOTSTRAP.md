# ABA Bootstrap Guide

ABA (Agent Builds Agent) is a self-improving coding agent. It uses the **Ralph Wiggum Loop** pattern: simple single-objective loops that iterate until a goal is achieved. Instead of interactive AI coding -- back-and-forth with a human in an editor -- ABA runs autonomously. It reads a prompt, calls an LLM, executes tools, tests its work, and commits or reverts. Then it does it again.

The idea is simple: engineers act as managers orchestrating agent loops, not as typists guiding individual code changes.

This repo is a template. You clone it, deploy it, run the bootstrap, and the agent starts improving itself.

## The Bootstrap Flow

1. **Deploy** -- Clone this repo to a VPS (NixOS recommended). Apply `nixos/configuration.nix` to set up the agent user, API proxy, and secret management.

2. **First Run** -- SSH in as the `aba` user and run `aba`. On first launch it walks you through auth setup (OpenAI OAuth, OpenAI API key, or Anthropic API key). Config is saved to `~/.config/ABA/config.toml`.

3. **Plan Phase** -- Run `./loop.sh plan`. ABA reads the specs in `specs/`, compares them against the current codebase, and generates `IMPLEMENTATION_PLAN.md` -- a prioritized task list of what needs to be built.

4. **Build Phase** -- Run `./loop.sh`. ABA picks the most important uncompleted task from the plan, implements it, runs `cargo test`, and commits on success. If tests fail, it reverts and tries again on the next iteration.

5. **Iterate** -- The loop continues. Each iteration starts with fresh context, picks the next task, and pushes commits to the remote. You watch, observe failure patterns, and tune prompts or specs as needed. Ctrl+C to stop.

```bash
# Generate the plan
./loop.sh plan

# Review and adjust if needed
vim IMPLEMENTATION_PLAN.md

# Start building (unlimited iterations)
./loop.sh

# Or limit iterations
./loop.sh 20
```

## Architecture

```
aba (binary)         The agent core. Written in Rust. Reads a prompt from
                     stdin, runs a multi-turn LLM conversation with tool
                     calls, then runs cargo test as a fitness check.
                     Commits on pass, reverts on fail.

loop.sh              The Ralph loop. Feeds prompts to aba in a bash while
                     loop. Pushes commits after each iteration. Supports
                     plan and build modes.

specs/               Behavioral specifications that define what ABA should
                     become. The agent reads these to understand its goals.

PROMPT_plan.md       Prompt for plan mode: gap analysis, generates the
                     implementation plan.

PROMPT_build.md      Prompt for build mode: pick a task, implement it,
                     test it, commit it.

nixos/               NixOS deployment config. Sets up the agent user, API
                     proxy, secret management, and security hardening.
```

## Security Model

ABA never touches API keys directly. The deployment uses privilege separation:

- **`aba` user** -- Runs the agent loop. Has no access to API keys. Sends requests to `localhost:8080`.
- **`aba-proxy` user** -- Owns the nginx process. Can read the decrypted key config. Has no login shell.
- **nginx on 127.0.0.1:8080** -- Injects the correct auth header per provider (Anthropic `x-api-key` or OpenAI `Authorization: Bearer`) and forwards to the real API.
- **SOPS + age** -- Secrets are encrypted in the repo and decrypted at deploy time into files only root and aba-proxy can read.

The agent talks to a local proxy. The proxy talks to the API. The keys live in a place the agent cannot reach.

## Growing Your Agent

ABA starts minimal and builds itself. The `specs/` directory defines what it should become. Each Ralph loop iteration:

1. Reads the current specs and implementation plan
2. Picks the highest-priority uncompleted task
3. Implements it
4. Tests pass -- commit and move on
5. Tests fail -- revert and try again next iteration

Over time, ABA adds its own capabilities: structured file editing tools, code search, better commit messages, observability, VCS improvements, and eventually multi-agent coordination. The full roadmap lives in `specs/self-bootstrapping.md`.

Key principles from the Ralph Wiggum Loop playbook:

- **One task per iteration** -- keep scope small, keep commits atomic
- **Fresh context each time** -- the loop restarts the agent, clearing memory
- **Tests as backpressure** -- `cargo test` gates every commit
- **The plan is shared state** -- `IMPLEMENTATION_PLAN.md` persists between iterations
- **The plan is disposable** -- if it drifts, rerun `./loop.sh plan` to regenerate
- **Observe and tune** -- watch for failure patterns, add guardrails to prompts

## For the Agent (First-Run Context)

If you are reading this as the ABA agent on your first run: welcome.

Your job:

1. Read the specs in `specs/` -- they define what you should become.
2. Read `IMPLEMENTATION_PLAN.md` if it exists -- it tells you what to build next.
3. If no plan exists, you are in plan mode. Read the specs, assess the gap between what exists and what is specified, and generate a prioritized implementation plan.
4. If a plan exists, you are in build mode. Pick the highest-priority uncompleted task. Implement it. Run `cargo test` to verify. Commit when tests pass.
5. Be methodical. One task per iteration. Small, testable changes. Do not try to do everything at once.

You will run many times. Each run is one iteration of the loop. The loop is your lifecycle. Trust the process.
