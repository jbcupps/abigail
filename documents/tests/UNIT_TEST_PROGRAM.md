# Unit Test Program (Current)

Date: 2026-03-02

## Objective

Validate logic-level correctness for routing, skills, policy, memory, and command contracts without requiring live external services.

## Core Coverage Areas

- `UNIT-ROUTER`: routing modes, selection reasons, execution trace attribution, force override precedence
- `UNIT-SKILLS`: sandbox, manifest parsing, executor behavior, trust policy
- `UNIT-CORE`: config, verifier, secrets, vault, capability envelope behavior
- `UNIT-MEMORY`: store/archive/graph behavior and persistence invariants
- `UNIT-UI`: frontend unit/component tests and chat gateway parity tests

## Required Commands

1. `cargo test --workspace --exclude abigail-app`
2. `cargo clippy --workspace --exclude abigail-app -- -D warnings`
3. `cargo check -p abigail-app`
4. `cd tauri-app/src-ui && npm run test:coverage`

## Priority Test Cases

### UNIT-ROUTER-001
- Title: Tier routing emits valid `SelectionReason` and trace attribution
- Priority: P0
- Evidence: `abigail-router` tests, `ChatResponse.execution_trace` assertions

### UNIT-ROUTER-002
- Title: `ForceOverride.pinned_model` and `pinned_tier` precedence is deterministic
- Priority: P0
- Evidence: `abigail-router` routing tests

### UNIT-ROUTER-003
- Title: `cli_orchestrator` bypasses tier scoring and complexity classification
- Priority: P0
- Evidence: `abigail-router` and provider selection tests

### UNIT-SKILLS-001
- Title: Skill trust policy enforces allowlist/signature gates
- Priority: P0
- Evidence: `abigail-skills` policy and executor tests

### UNIT-SKILLS-002
- Title: MCP trust policy rejects disallowed hosts before request dispatch
- Priority: P0
- Evidence: `abigail-skills` MCP protocol tests

### UNIT-UI-001
- Title: Chat gateway parity normalizes behavior across adapters
- Priority: P0
- Evidence: `tauri-app/src-ui/src/chat/__tests__/ChatGateway.parity.test.ts`

## Redundant Test Policy

The following are marked redundant and should be removed in a future code-cleanup PR:

- Duplicate frontend lifecycle assertions that overlap `App.browserFlow` and `App.lifecycle` without adding new state coverage.
- Legacy suite-ID-only test bookkeeping (`BIRTH-*`, `CRYS-*`, `OPER-*`) when coverage is already represented by `UNIT-*` or `UX-*` IDs.

Until removal lands in code, these tests are non-canonical and do not block release decisions by themselves.

## Exit Criteria

- All P0 unit cases pass.
- No unresolved P0 unit regressions in routing, trust policy, or gateway parity.
