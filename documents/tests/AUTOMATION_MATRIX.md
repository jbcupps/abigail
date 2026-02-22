# Automation Matrix

## Legend
- `AutomatedHarness`: covered by browser harness automation (Vitest)
- `ManualNative`: must run in native Tauri window
- `Hybrid`: partial automation + required manual parity step
- `Deferred`: not yet automatable with current harness

## Matrix

| Test ID | Suite | Execution Mode | Current Coverage | Notes |
|---|---|---|---|---|
| BIRTH-001 | Birth | AutomatedHarness | Covered | `App.browserFlow` |
| BIRTH-002 | Birth | AutomatedHarness | Covered | `App.browserFlow` |
| BIRTH-003 | Birth | AutomatedHarness | Covered | `App.browserFlow` |
| BIRTH-004 | Birth | AutomatedHarness | Covered | `App.browserFlow` |
| BIRTH-005 | Birth | Hybrid | Partial | Add explicit blocking assertion variant |
| BIRTH-006 | Birth | ManualNative | Manual | Recovery path parity |
| CRYS-001 | Crystallization | AutomatedHarness | Covered | `App.browserFlow` |
| CRYS-002 | Crystallization | AutomatedHarness | Covered | `App.browserFlow` |
| CRYS-003 | Crystallization | AutomatedHarness | Pending | Add validation test |
| CRYS-004 | Crystallization | AutomatedHarness | Covered | `App.browserFlow` |
| CRYS-005 | Crystallization | Hybrid | Manual-heavy | Visual persistence parity |
| CRYS-006 | Crystallization | ManualNative | Manual | Non-fast path |
| OPER-001 | Operational | AutomatedHarness | Covered | `App.browserFlow` + chat tests |
| OPER-002 | Operational | AutomatedHarness | Covered | clipboard response test |
| OPER-003 | Operational | Hybrid | Pending | Add explicit routing telemetry assertion |
| OPER-004 | Operational | ManualNative | Manual | Forge/Registry stability |
| OPER-005 | Operational | Hybrid | Pending | Session continuity parity |
| SKILL-001 | Skills | AutomatedHarness | Covered | clipboard smoke path |
| SKILL-002 | Skills | Hybrid | Pending | missing-secret behavior path |
| SKILL-003 | Skills | AutomatedHarness | Covered | `HarnessDebug` fault tests |
| SKILL-004 | Skills | ManualNative | Manual | skill creation discoverability |
| SKILL-005 | Skills | ManualNative | Manual/Deferred | lifecycle controls may not be exposed |
| SPAWN-001 | Spawning | Hybrid | Pending | requires harness flow extension |
| SPAWN-002 | Spawning | Hybrid | Pending | context switch assertions |
| SPAWN-003 | Spawning | ManualNative | Manual | archive behavior in native registry |
| SPAWN-004 | Spawning | Hybrid | Pending | active archive guard assertion |
| SPAWN-005 | Spawning | ManualNative | Manual | delete behavior parity |
| SPAWN-006 | Spawning | ManualNative | Manual | isolation sanity check |

## Near-term Automation Additions (Priority)

1. `CRYS-003` — required-name validation test
2. `BIRTH-005` — connectivity block without provider configured
3. `OPER-003` — routing telemetry assertion under debug mode
4. `SPAWN-001/002/004` — harness-level lifecycle tests

