# UX Test Program (Current)

Date: 2026-10-21

## Objective

Validate user-visible behavior in the two framework apps: Hive (model connection, Entity creation, dashboard) and the Entity Runtime (chat, durable history, failure recovery).

## Canonical UX Suites

- `UX-HIVE`: Hive launch, model connection, and Entity creation
- `UX-CHAT`: Entity Runtime chat rendering, streaming, and durable conversations
- `UX-STYLE`: embedded stylesheet defaults for both apps

## Required Commands

Run in both `hive-app/src-ui` and `entity-runtime-app/src-ui`:

1. `npm run build`
2. `npm test`

## Priority Test Cases

### UX-HIVE-001
- Title: Hive opens from splash into the dashboard
- Priority: P0
- Evidence: `hive-app/src-ui/src/App.test.tsx`

### UX-HIVE-002
- Title: Model connection verifies a real provider (installed CLI, local server, or cloud key) before reporting success
- Priority: P0
- Evidence: `hive-app/src-ui/src/components/ProviderWizard.test.tsx`

### UX-HIVE-003
- Title: Entity creation records the family's purpose and retries without duplicating a saved Entity
- Priority: P0
- Evidence: `hive-app/src-ui/src/components/CreateEntityCard.test.tsx`

### UX-CHAT-001
- Title: Entity Runtime opens from splash into chat
- Priority: P0
- Evidence: `entity-runtime-app/src-ui/src/App.test.tsx`

### UX-CHAT-002
- Title: Named history is restored, replies stream, and the same conversation resumes after reopening
- Priority: P0
- Evidence: `entity-runtime-app/src-ui/src/components/ChatPanel.test.tsx`

### UX-CHAT-003
- Title: History and stream failures are surfaced without silently replaying messages or tools
- Priority: P0
- Evidence: `entity-runtime-app/src-ui/src/components/ChatPanel.test.tsx`

### UX-STYLE-001
- Title: Embedded stylesheet supplies default colors, spacing, and type scale
- Priority: P1
- Evidence: `styles.test.ts` in both apps

## Deprecated UX IDs

The old UX planning IDs are deprecated and mapped as follows:

- `BIRTH-*`, `CRYS-*`, `OPER-*`, `UX-APP-*` -> `UX-HIVE-*`
- `SKILL-*` UX-only checks, `UX-HARNESS-*` -> `UX-CHAT-*`
- `UX-SANCTUM-*` -> retired with the single-agent app (`legacy/tauri-app`)
- `SPAWN-*` UI lifecycle checks -> `E2E-AGENT-*` (runtime-owned)

## Exit Criteria

- All P0 UX cases pass.
- No unresolved P0 UI regression in Hive launch, model connection, Entity creation, or chat send/render flow.
