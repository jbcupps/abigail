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
| STAB-001 | Message Flow Stability | AutomatedHarness | Pending | command surface contract gate |
| STAB-010 | Message Flow Stability | Hybrid | Pending | adapter parity: tauri vs entity |
| STAB-020 | Message Flow Stability | AutomatedHarness | Pending | stream token/done/error sequencing |
| STAB-030 | Message Flow Stability | Hybrid | Pending | agent lifecycle happy path |
| STAB-035 | Message Flow Stability | ManualNative | Pending | restart recovery |
| STAB-040 | Message Flow Stability | Hybrid | Pending | entity-initiated run trigger |
| STAB-050 | Message Flow Stability | AutomatedHarness | Pending | runtime subagent registry presence |
| STAB-060 | Message Flow Stability | Hybrid | Pending | MCP trust enforcement |
| STAB-062 | Message Flow Stability | Hybrid | Pending | signed allowlist accept path (trusted signer) |
| STAB-063 | Message Flow Stability | Hybrid | Pending | policy deny path for unsigned/unallowlisted skill |
| STAB-064 | Message Flow Stability | Hybrid | Pending | signed allowlist enforcement |

## Near-term Automation Additions (Priority)

1. `CRYS-003` — required-name validation test
2. `BIRTH-005` — connectivity block without provider configured
3. `OPER-003` — routing telemetry assertion under debug mode
4. `SPAWN-001/002/004` — harness-level lifecycle tests
5. `STAB-001` — command contract CI gate
6. `STAB-010..014` — chat adapter parity coverage
7. `STAB-030..036` — agent lifecycle + recovery
