# UX Test Program (Current)

Date: 2026-03-02

## Objective

Validate user-visible behavior in desktop/browser-harness flows: birth, provider setup, crystallization, chat, diagnostics, and failure recovery.

## Canonical UX Suites

- `UX-APP`: full app lifecycle and key state transitions
- `UX-CHAT`: rendering, message metadata, and gateway-facing behavior
- `UX-HARNESS`: browser-harness debug controls and fault simulation
- `UX-SANCTUM`: settings/management panel behavior

## Required Commands

1. `cd tauri-app/src-ui && npm run build`
2. `cd tauri-app/src-ui && npm run test:coverage`
3. `cd tauri-app/src-ui && npm run check:command-contract`

## Priority Test Cases

### UX-APP-001
- Title: Birth -> connectivity -> crystallization -> emergence happy path
- Priority: P0
- Evidence: `App.browserFlow.test.tsx`

### UX-APP-002
- Title: Connectivity guard blocks progression with no configured provider
- Priority: P0
- Evidence: `App.browserFlow.test.tsx`

### UX-APP-003
- Title: Provider validation failure surfaces actionable message and recovery path
- Priority: P0
- Evidence: `App.browserFlow.test.tsx`

### UX-CHAT-001
- Title: Chat rendering preserves routing/provider metadata expectations
- Priority: P1
- Evidence: `ChatRendering.test.tsx`, `ChatInterface.test.tsx`

### UX-HARNESS-001
- Title: Harness fault injection reports errors and supports retry recovery
- Priority: P1
- Evidence: `HarnessDebug.test.tsx`

### UX-SANCTUM-001
- Title: Sanctum tab fallback behavior is deterministic when staff tab is unavailable
- Priority: P1
- Evidence: `SanctumDrawer.test.tsx`

## Deprecated UX IDs

The old UX planning IDs are deprecated and mapped as follows:

- `BIRTH-*`, `CRYS-*`, `OPER-*` -> `UX-APP-*`
- `SKILL-*` UX-only checks -> `UX-CHAT-*` or `UX-HARNESS-*`
- `SPAWN-*` UI lifecycle checks -> `E2E-AGENT-*` (runtime-owned)

## Exit Criteria

- All P0 UX cases pass.
- No unresolved P0 UI regression in boot, provider setup, or chat send/render flow.
