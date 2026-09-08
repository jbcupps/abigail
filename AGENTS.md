# AGENTS.md — Active Implementation Plan for All Coding Agents

This file tracks the current plan for agents working in the Abigail repository.

## Current State (2026-09-08)

Abigail is the private Entity Coordinator and Manager for real homes and families. Its product mission is to put persistent, highly configurable, ethically governed AI Entities into the hands of everyday people through one easy install and one obvious app launch. The user (mentor/family head) creates individual Entities that the family actually interacts with. Abigail handles coordination, memory, skills, and security in the background.

- The coordinator is called **Abigail** in the family-facing experience. Provider/model management stays there; Hive remains the internal component name.
- Abigail Hive should remain visible and usable even while an Entity is open.
- The active implementation split is two app roots: `hive-app` for control-plane work and `entity-runtime-app` for the chat/runtime surface, but the family-facing product must install and launch as one `Abigail` app.
- The full installer lane must package Hive, Entity Runtime, `hive-daemon`, and `entity-daemon` together so non-technical families never handle separate binaries.
- `beta` is the permanent primary iteration branch. Agents should target PRs and automation at `beta` for UAT builds, then promote validated beta changes to `main`.
- Every push to `beta` must produce a properly tagged beta installer prerelease for UAT using the `vX.Y.Z-beta.N` tag pattern.
- Mentor chat monitor preprompt flow is in place and out-of-band monitors remain non-blocking.
- DevOps Forge worker is active and subscribed to `topic.skill.forge.request`.
- Forge pipeline writes sandbox-gated artifacts to `skills/dynamic/`, updates `skills/registry.toml`, and publishes `topic.skill.forge.response`.

## Initial Application Setup

- The splash leads to verified local inference from packaged Ollama and Qwen3.5-0.8B; no first-run download is required.
- Abigail's focused setup chat runs inside the coordinator under its immortal identity and persists in its Entity database. Family runtimes share the Hive-owned store through authenticated scoped requests.
- Connecting Claude or OpenAI requires a secure form and successful completion from the selected model before an encrypted connection is persisted and activated. The same conversation survives handoff and restart.
- All privileged local APIs require caller credentials. A separate credential is passed directly to each Runtime window; Entity Hive tokens cannot read another Entity's records or change global setup.
- Do not expand this slice into household accounts or a new Entity birth conversation. Those remain subsequent product work.
- Keep unsigned packaging reproducible with the offline runtime/model/licenses and offline WebView2. Full disconnected installer UAT remains required before release; use `docs/INITIAL_SETUP.md` for acceptance commands.

## Active Plan (Family-First Priorities)

1. Keep the always-open Hive shell and Hive-owned model management stable and test-backed.
2. Use `beta` as the primary integration and UAT branch; keep `main` as the promoted stable release branch.
3. Keep the one-step installer and one-app launch path repeatable, tested, and aligned with the split runtime architecture.
4. Harden Forge envelope validation and failure telemetry (keep it invisible and safe for the user).
5. Expand end-to-end coverage for forge request/response and watcher hot-reload.
6. Keep memory/safety/id-superego observers out-of-band (non-blocking chat path) so the family experience stays smooth.
7. Keep the unsigned stabilization lane free of installer upgrade-preserve logic, updater assumptions, and Windows signing dependencies.

## Definition of Done for Next Phase

- Forge request envelope accepts code + markdown and persists deterministically.
- Windows installer installs Abigail as one app and bundles the split Hive/Runtime daemons as internal resources.
- Merges to `beta` automatically create tagged beta prereleases for UAT; stable releases come only from promoted `main`.
- Superego and sandbox gates prevent unsafe mutations while staying invisible to the user.
- Registry update reliably triggers watcher-based hot-reload.
- End-to-end coverage validates success, blocked, and error fallback behavior.
- Legacy compatibility paths that conflict with the current dev UX are removed instead of preserved.

## Documentation Sync

When changing routing or monitor behavior, update:
- `README.md` (user-facing family story)
- `CLAUDE.md` and `AGENTS.md` (agent constitution / active plan files)

**Remember the Mission**: Abigail coordinates the Entities that families actually talk to. Every change must make the experience warmer, simpler, and more powerful for real homes.

Application-owned setup guidance must remain visible independently of model prose:
API accounts are provider developer accounts; API keys are separate secrets created
inside those accounts. Only fixed official provider links and the secure connection
form direct credential entry. The live connection status and next setup action come
from application state. Qwen3.5-0.8B free-form explanations can confuse these terms,
even with examples; never treat its wording as proof of setup completion.
