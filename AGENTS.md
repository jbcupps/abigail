# AGENTS.md — Active Implementation Plan for All Coding Agents

This file tracks the current plan for agents working in the Abigail repository.

## Current State (2026-03-05)

Abigail is the private Entity Coordinator and Manager for real homes and families. The user (mentor/family head) creates individual Entities that the family actually interacts with. Abigail handles coordination, memory, skills, and security in the background.

- Abigail Hive is the only place where provider/model management should live.
- Abigail Hive should remain visible and usable even while an Entity is open.
- The active stabilization split is two app roots: `hive-app` for control-plane work and `entity-runtime-app` for the chat/runtime surface.
- Mentor chat monitor preprompt flow is in place and out-of-band monitors remain non-blocking.
- DevOps Forge worker is active and subscribed to `topic.skill.forge.request`.
- Forge pipeline writes sandbox-gated artifacts to `skills/dynamic/`, updates `skills/registry.toml`, and publishes `topic.skill.forge.response`.

## Active Plan (Family-First Priorities)

1. Keep the always-open Hive shell and Hive-owned model management stable and test-backed.
2. Harden Forge envelope validation and failure telemetry (keep it invisible and safe for the user).
3. Expand end-to-end coverage for forge request/response and watcher hot-reload.
4. Keep memory/safety/id-superego observers out-of-band (non-blocking chat path) so the family experience stays smooth.
5. Keep the unsigned stabilization lane free of installer upgrade-preserve logic, updater assumptions, and Windows signing dependencies.

## Definition of Done for Next Phase

- Forge request envelope accepts code + markdown and persists deterministically.
- Superego and sandbox gates prevent unsafe mutations while staying invisible to the user.
- Registry update reliably triggers watcher-based hot-reload.
- End-to-end coverage validates success, blocked, and error fallback behavior.
- Legacy compatibility paths that conflict with the current dev UX are removed instead of preserved.

## Documentation Sync

When changing routing or monitor behavior, update:
- `README.md` (user-facing family story)
- `CLAUDE.md` and `AGENTS.md` (agent constitution / active plan files)

**Remember the Mission**: Abigail coordinates the Entities that families actually talk to. Every change must make the experience warmer, simpler, and more powerful for real homes.
