---
name: abigail-dev-loop
description: Verify and drive the Abigail Rust, Tauri, and React workspace through a code-first local development loop. Use when work involves checking whether Abigail still works, inspecting the GUI chat path, hive or entity daemons, HTTP or SSE routes, gateways, configs, build or test scripts, or creating or repairing Abigail skills with repeatable bridge scripts and conversational test prompts. Use for GUI-versus-daemon parity analysis, human-supervised local dev instances, and end-to-end skill validation in this repository.
---

# Abigail Dev Loop

## Mission

Use this skill only inside the Abigail repository. Inspect the repository before making architectural claims, verify behavior from source and targeted checks, and leave behind a repeatable local loop that both Codex and a human developer can use.

## Evidence Rules

- Label statements as `Verified`, `Likely`, or `Unknown`.
- Prefer source files, tests, and commands you ran over old prose.
- Do not assume ports, routes, crate names, package manager commands, or watcher behavior.
- If a claimed endpoint or interface is absent, say so plainly and propose the smallest viable implementation.

## First Pass

1. Read repo intent and runbook first:
   - `AGENTS.md`
   - `CLAUDE.md`
   - `README.md`
   - `documents/HOW_TO_RUN_LOCALLY.md`
   - `Cargo.toml`
   - `tauri-app/src-ui/package.json`
2. Inspect the current runtime surfaces:
   - `crates/hive-daemon/src/main.rs`
   - `crates/hive-daemon/src/routes.rs`
   - `crates/entity-daemon/src/main.rs`
   - `crates/entity-daemon/src/routes.rs`
   - `crates/entity-cli/src/main.rs`
   - `crates/daemon-client/src/entity.rs`
   - `crates/daemon-test-harness/src/lib.rs`
   - `scripts/check_command_surface.mjs`
3. Inspect tests and reports closest to the task:
   - `documents/GUI_ENTITY_CODE_REVIEW_REPORT.md`
   - `documents/DYNAMIC_API_SKILL_AUTHORING.md`
   - `documents/tests/`
   - targeted tests under `crates/**/tests`
4. Build an evidence table before changing code:
   - runtime topology
   - GUI transport
   - daemon transport
   - streaming and cancel support
   - skill loading and watcher triggers
   - current health and test status

## Verified Repo Anchors

At the time this skill was authored, these repo facts were verified from source and should be re-checked on every use:

- The Rust workspace includes `hive-daemon`, `entity-daemon`, `entity-cli`, `daemon-client`, `daemon-test-harness`, and `tauri-app`.
- The frontend package lives in `tauri-app/src-ui` and exposes `dev`, `build`, `test`, `test:coverage`, and `check:command-contract`.
- `hive-daemon` defaults to `127.0.0.1:3141` and exposes `/health`, `/v1/status`, `/v1/entities`, `/v1/secrets/*`, and `/v1/providers/models`.
- `entity-daemon` defaults to `127.0.0.1:3142` and exposes `/health`, `/v1/status`, `/v1/chat`, `/v1/chat/stream`, `/v1/chat/cancel`, `/v1/routing/diagnose`, `/v1/skills`, `/v1/tools/execute`, `/v1/memory/*`, `/v1/jobs/*`, and `/v1/topics/:topic/watch`.
- `entity-cli` is the thin troubleshooting client for the entity daemon.
- `daemon-client` contains a reusable HTTP and SSE client that can back bridge scripts or tests.
- `daemon-test-harness` can boot ephemeral hive and entity daemons for automated local verification.
- The current skills watcher hot-reloads `registry.toml`, `skill.toml`, and `*.json`. It does not watch `SKILL.md` in `crates/abigail-skills/src/watcher.rs`.

## Verification Loop

1. Start with the least invasive useful checks.
2. Run only the checks that answer the current question, but report exactly what you ran and what passed or failed.
3. Prefer targeted commands such as:

```bash
cargo check -p hive-daemon
cargo check -p entity-daemon
cargo check -p entity-cli
cargo test -p entity-cli --test scaffold_discovery
cargo test -p abigail-skills watcher
cd tauri-app/src-ui && npm install
cd tauri-app/src-ui && npm run check:command-contract
cd tauri-app/src-ui && npm run build
cd tauri-app/src-ui && npm test
```

4. Escalate to broader checks only when the narrow checks are green or inconclusive:

```bash
cargo test --workspace --exclude abigail-app
```

5. When a check is too expensive, flaky, or blocked by missing secrets or GUI prerequisites, say so explicitly and mark the affected conclusions `Likely` or `Unknown`.

## Human-Supervised Dev Instance

- Prefer telling the human to run long-lived processes in separate terminals unless explicitly asked to run them yourself.
- After inspection, print exact commands for the current repo state. Reuse verified commands from `documents/HOW_TO_RUN_LOCALLY.md` if they still match the code.
- Default manual loop:

```bash
# Terminal 1
cargo run -p hive-daemon

# Terminal 2
cargo run -p hive-cli -- create "DevEntity"
cargo run -p entity-daemon -- --entity-id <uuid-from-hive-cli>

# Terminal 3
cargo run -p entity-cli -- status
cargo run -p entity-cli -- chat "hello"
cargo run -p entity-cli -- skills

# Terminal 4, browser harness path
cd tauri-app/src-ui
npm run dev

# Or native desktop path
cargo tauri dev
```

- Verify exact flags and startup order before claiming them in a final answer.

## Parity Matrix

When asked to compare GUI chat versus the troubleshooting or local daemon interface, produce a matrix with these rows:

- transport and session entry point
- request and response contract
- streaming mode and cancellation
- model override and provider selection
- persona, system prompt, and preprompt enrichment
- tool execution path and skill registry
- memory persistence and session threading
- diagnostics and routing introspection
- queue, jobs, and topic watch
- raw error visibility
- auth and localhost assumptions
- debug-only surfaces and browser harness behavior
- human usability versus automation friendliness

For each cell, include:

- status: `Verified`, `Likely`, or `Unknown`
- file evidence
- parity verdict: `Same`, `Different`, `Partial`, or `Missing`

## Bridge And Troubleshooting Scripts

When the user asks for a local bridge or conversational harness:

- Put new helpers under `scripts/dev/`, `scripts/qa/`, or another clearly repo-local path.
- Keep scripts idempotent where possible.
- Use ASCII only unless the file already requires otherwise.
- Prefer existing Abigail surfaces before inventing new ones:
  - `entity-cli` for simple chat, skills, and tool calls
  - `daemon-client` for Rust HTTP and SSE clients
  - `daemon-test-harness` for automated daemon bootstrapping
  - direct `curl` or PowerShell `Invoke-RestMethod` calls for raw route checks
- Cover both non-streaming and streaming paths when relevant:
  - `POST /v1/chat`
  - `POST /v1/chat/stream`
  - `POST /v1/chat/cancel`
  - `GET /v1/routing/diagnose`
  - `GET /v1/skills`
  - `POST /v1/tools/execute`
  - `GET /v1/topics/:topic/watch`
- After creating a script, run it or run the narrowest test that proves it works.

## New Abigail Skill Workflow

When repairing or building an Abigail runtime skill:

1. Decide whether it is:
   - a dynamic JSON skill under the entity or shared skills directories
   - a native Rust skill crate under `skills/`
   - an instruction-registry entry under `skills/instructions` plus `skills/registry.toml`
2. Inspect the existing patterns first:
   - `documents/DYNAMIC_API_SKILL_AUTHORING.md`
   - `skills/registry.toml`
   - one similar skill crate under `skills/`
   - watcher behavior in `crates/abigail-skills/src/watcher.rs`
3. Reuse built-in scaffolding when it fits:

```bash
cargo run -p entity-cli -- scaffold my-skill --type dynamic
cargo run -p entity-cli -- scaffold my-skill --type native
```

4. Validate in two ways:
   - direct tool path: invoke the tool via CLI or HTTP
   - indirect chat path: ask the entity to use the skill conversationally
5. If watcher parity is part of the task, verify actual hot-reload behavior from logs or tests. Do not assume markdown instruction edits reload automatically.

## Conversational Validation Suite

Always produce a conversational test suite with at least 10 prompts. Cover:

- handshake and health
- identity and persona grounding
- streaming behavior
- routing and diagnostic behavior
- tool use
- memory carryover
- failure and negative-trigger cases
- skill-specific success path
- skill-specific blocked or unsafe path
- repair-loop collaboration

For each prompt, include:

- the exact user message
- the target interface: GUI, `entity-cli`, raw HTTP, or both
- the expected success signal
- the likely failure signal
- any supporting command to observe logs or endpoint state

## Required Output

When using this skill, return all applicable items:

1. Build and health verdict with exact commands run and observed results.
2. Current behavior summary of the GUI app, daemons, and local communication loop.
3. GUI-versus-daemon parity matrix.
4. Manual dev-instance startup instructions for the human developer.
5. Any bridge or test scripts you created or updated, with run commands.
6. A conversational test suite with at least 10 prompts.
7. A repair loop plan when checks fail.
8. An unresolved uncertainties list.

## Failure Handling

- If the build or targeted tests fail, narrow the breakage to the smallest boundary first: manifest, compile, route contract, gateway, skill load, watcher, or UI command surface.
- Prefer the smallest viable patch that restores the loop.
- Explain what changed, why it changed, and how to verify it.
- If the repo lacks the claimed interface, say that the interface is absent and propose the minimum path to add it.
