# AGENTS.md

This file tracks the active implementation plan for coding agents working in this repository.

## Current State (2026-03-05)

- Provider/model selector propagation is wired end-to-end.
- Mentor chat monitor preprompt flow is in place and out-of-band monitors remain non-blocking.
- DevOps Forge worker is active in `entity-daemon` and subscribed to `topic.skill.forge.request`.
- Forge pipeline now writes sandbox-gated artifacts to `skills/dynamic/`, updates `skills/registry.toml`, and publishes `topic.skill.forge.response`.

## Active Plan

1. Keep selector + mentor monitor paths stable and test-backed.
2. Harden Forge envelope validation and failure telemetry.
3. Expand end-to-end coverage for forge request/response and watcher hot-reload.
4. Keep memory/safety/id-superego observers out-of-band (non-blocking chat path).

## Definition of Done for Next Phase

- Forge request envelope accepts code + markdown and persists deterministically.
- Superego and sandbox gates prevent unsafe forge mutations.
- Registry update reliably triggers watcher-based hot-reload.
- End-to-end coverage validates success, blocked, and error fallback behavior.

## Documentation Sync

When changing routing or monitor behavior, update:

- `README.md` (roadmap and current plan)
- `CLAUDE.md` (developer architecture + roadmap)
- `docs/architecture.md` and matching copies under `documents/` if flow diagrams change
