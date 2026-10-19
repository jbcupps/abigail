# AGENTS.md — Active Implementation Plan for All Coding Agents

This file tracks the current plan for agents working in the Abigail repository.

## Current State (2026-10-19)

Abigail is the private Entity Coordinator and Manager for real homes and families. Its product mission is to put persistent, highly configurable, ethically governed AI Entities into the hands of everyday people through one easy install and one obvious app launch. The user (mentor/family head) creates individual Entities that the family actually interacts with. Abigail handles coordination, memory, skills, and security in the background.

- Abigail Hive is the only place where provider/model management should live.
- Abigail Hive should remain visible and usable even while an Entity is open.
- The active implementation split is two app roots: `hive-app` for control-plane work and `entity-runtime-app` for the chat/runtime surface, but the family-facing product must install and launch as one `Abigail` app.
- The full installer lane must package Hive, Entity Runtime, `hive-daemon`, and `entity-daemon` together so non-technical families never handle separate binaries.
- `beta` is the permanent primary iteration branch. Agents should target PRs and automation at `beta` for UAT builds, then promote validated beta changes to `main`.
- Every push to `beta` must produce a properly tagged beta installer prerelease for UAT using the `vX.Y.Z-beta.N` tag pattern.
- Mentor chat monitor preprompt flow is in place and out-of-band monitors remain non-blocking.
- DevOps Forge worker is active and subscribed to `topic.skill.forge.request`.
- The Forge broker strictly validates bounded proposals and publishes `topic.skill.forge.response`; `validated` means static checks passed, with `applied=false`, `fitness_evaluated=false`, and `hot_reload_triggered=false`.
- Broker proposals cannot grant approval, permissions, scores, or adoption. The direct writer requires non-deserializable `ForgeWriteAuthorization` from a trusted host caller; no authenticated approval route or behavioral adoption/rollback loop is connected to it.
- `builtin.skill_factory` remains a separate authoring path. Forge validation does not govern all capability creation.
- New Forge and memory subscriber diagnostic records contain fixed codes and bounded byte counts only. Historical logs are unchanged.

## Active Plan (Family-First Priorities)

1. Keep the always-open Hive shell and Hive-owned model management stable and test-backed.
2. Use `beta` as the primary integration and UAT branch; keep `main` as the promoted stable release branch.
3. Keep the one-step installer and one-app launch path repeatable, tested, and aligned with the split runtime architecture.
4. Preserve strict Forge validation and content-free failure telemetry (keep it invisible and safe for the user).
5. Add host-owned behavioral evaluation and authenticated application before claiming Forge adoption; test watcher activation separately from proposal validation.
6. Keep memory/safety/id-superego observers out-of-band (non-blocking chat path) so the family experience stays smooth.
7. Keep the unsigned stabilization lane free of installer upgrade-preserve logic, updater assumptions, and Windows signing dependencies.

## Definition of Done for Next Phase

- Forge request/response tests cover malformed and authority-bearing proposals and verify that static validation never mutates skills or the registry.
- Future Forge adoption requires host-owned baseline comparisons, rejection of no gain and protected-case regressions, and honest application/rollback records; these are not implemented by static validation.
- Windows installer installs Abigail as one app and bundles the split Hive/Runtime daemons as internal resources.
- Merges to `beta` automatically create tagged beta prereleases for UAT; stable releases come only from promoted `main`.
- Keep static Superego checks and host write permissions separate; passing a pattern scan is not proof that generated code is safe.
- Registry update reliably triggers watcher-based hot-reload.
- End-to-end coverage distinguishes validated, blocked, and error outcomes from application and activation.
- Legacy compatibility paths that conflict with the current dev UX are removed instead of preserved.

## Documentation Sync

When changing routing or monitor behavior, update:
- `README.md` (user-facing family story)
- `CLAUDE.md` and `AGENTS.md` (agent constitution / active plan files)

**Remember the Mission**: Abigail coordinates the Entities that families actually talk to. Every change must make the experience warmer, simpler, and more powerful for real homes.
