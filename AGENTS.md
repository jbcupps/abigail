# AGENTS.md

This file tracks the active implementation plan for coding agents working in this repository.

## Current State (2026-03-05)

- Provider/model selector propagation is wired end-to-end.
- `store_provider_key` emits `provider-config-changed` after key storage.
- `ChatInterface` listens for `provider-config-changed`, refreshes router status, and re-fetches model registry.
- Header provider selector auto-defaults when providers become available.
- Regression coverage exists in `ChatInterface.test.tsx` for the provider-config refresh path.

## Active Plan

1. Keep selector path stable and test-backed.
2. Implement Mentor Chat Monitor subscription in router flow.
3. Add monitor-based preprompt injection before ego completion.
4. Keep memory/safety/id-superego observers out-of-band (non-blocking chat path).

## Definition of Done for Next Phase

- Mentor message produces a chat-topic envelope consumed by the monitor.
- Preprompt is injected deterministically and visible in debug traces.
- Monitor pipeline failures degrade gracefully without breaking chat completion.
- End-to-end test coverage validates subscription, preprompt, and fallback behavior.

## Documentation Sync

When changing routing or monitor behavior, update:

- `README.md` (roadmap and current plan)
- `CLAUDE.md` (developer architecture + roadmap)
- `docs/architecture.md` and matching copies under `documents/` if flow diagrams change
