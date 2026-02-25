# Abigail UI Test Program Index

## Scope

This program defines five executable UI test suites:
- Birth
- Crystallization
- Operational Hive Interface
- Skills
- Agent Spawning

Primary references:
- `documents/BROWSER_HARNESS_QA_PROTOCOL.md`
- `tauri-app/src-ui/src/__tests__/App.browserFlow.test.tsx`
- `tauri-app/src-ui/src/__tests__/HarnessDebug.test.tsx`

## Shared Test Case Schema

Every test case in all suite documents must use this structure:

- `Test ID`: unique suite-prefixed identifier
- `Title`
- `Priority`: `P0`, `P1`, `P2`
- `Type`: `AutomatedHarness`, `ManualNative`, `Hybrid`
- `Preconditions`
- `Steps` (numbered)
- `Expected`
- `Evidence`
- `Result`: `Pass`, `Fail`, `Blocked`
- `Defect / Notes`

## ID Ranges

- `BIRTH-001..099`
- `CRYS-001..099`
- `OPER-001..099`
- `SKILL-001..099`
- `SPAWN-001..099`

## Severity / Triage

- `Critical`: release-blocking, no workaround
- `High`: core flow broken, workaround weak
- `Medium`: non-core behavior or recoverable failure
- `Low`: minor UX/documentation/cosmetic gap

## Execution Order

1. Birth Suite
2. Crystallization Suite
3. Operational Suite
4. Skills Suite
5. Agent Spawning Suite
6. Regression + Native parity gates

## Automation Matrix (Program-Level)

| Suite | Automated Harness | Manual Native | Hybrid |
|---|---:|---:|---:|
| Birth | Yes | Yes | Yes |
| Crystallization | Yes | Yes | Yes |
| Operational | Yes | Yes | Yes |
| Skills | Partial | Yes | Yes |
| Agent Spawning | Partial | Yes | Yes |

Notes:
- Skills creation and deep spawning lifecycle may require harness extensions for full automation parity.
- Native parity remains mandatory for release `Go`.

## Program Gates

- `Gate-A`: All P0 automated harness tests pass.
- `Gate-B`: `npm run build` passes.
- `Gate-C`: `npm run test:coverage` passes.
- `Gate-D`: Native parity smoke checks pass.
- `Gate-E`: No unresolved P0 `Blocked` tests.

## Suite Documents

- `documents/tests/BIRTH_UI_TEST_PLAN.md`
- `documents/tests/CRYSTALLIZATION_UI_TEST_PLAN.md`
- `documents/tests/OPERATIONAL_HIVE_UI_TEST_PLAN.md`
- `documents/tests/SKILLS_TEST_PLAN.md`
- `documents/tests/AGENT_SPAWNING_TEST_PLAN.md`
- `documents/tests/AUTOMATION_MATRIX.md`
- `documents/tests/VALIDATION_AND_GATE_REPORT.md`

## Integration Test Guides

- `documents/tests/EMAIL_INTEGRATION_TEST_GUIDE.md` — Step-by-step instructions for running the live email E2E tests (`entity-chat` crate, env-gated)
- `documents/tests/EMAIL_INTEGRATION_REPORT.md` — Detailed report of email integration test results, bugs found, and fixes applied

