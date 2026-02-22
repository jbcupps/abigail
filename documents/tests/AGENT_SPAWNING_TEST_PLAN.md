# Agent Spawning Test Plan

## Objective

Validate multi-agent lifecycle operations: create, load/switch, archive, delete, and isolation expectations.

## Test Cases

### SPAWN-001
- **Title**: Create first and second agents
- **Priority**: P0
- **Type**: Hybrid
- **Preconditions**: Management screen
- **Steps**:
  1. Create agent A.
  2. Return to management.
  3. Create agent B.
- **Expected**: Both appear in registry and are independently loadable.
- **Evidence**: Registry capture with both identities.

### SPAWN-002
- **Title**: Switch active agent context
- **Priority**: P0
- **Type**: Hybrid
- **Preconditions**: At least two agents present
- **Steps**:
  1. Load agent A and check identity cues.
  2. Disconnect and load agent B.
- **Expected**: Active context updates correctly without cross-identity confusion.
- **Evidence**: agent identity indicators before/after switch.

### SPAWN-003
- **Title**: Archive non-active agent
- **Priority**: P0
- **Type**: ManualNative
- **Preconditions**: Two agents present, one inactive
- **Steps**:
  1. Archive inactive agent.
  2. Verify it is removed from active registry list.
- **Expected**: Archive succeeds and remaining active agent is intact.
- **Evidence**: archive confirmation + registry state.

### SPAWN-004
- **Title**: Active-agent archive guard
- **Priority**: P1
- **Type**: Hybrid
- **Preconditions**: Agent currently active
- **Steps**:
  1. Attempt archive on active agent.
- **Expected**: Operation blocked with clear message (must suspend first).
- **Evidence**: error message capture.

### SPAWN-005
- **Title**: Delete non-active agent lifecycle
- **Priority**: P1
- **Type**: ManualNative
- **Preconditions**: Non-active agent present
- **Steps**:
  1. Delete selected non-active agent.
  2. Verify removal from registry.
- **Expected**: Deletion succeeds with no corruption of other identities.
- **Evidence**: confirmation + post-delete registry.

### SPAWN-006
- **Title**: Isolation sanity check
- **Priority**: P2
- **Type**: ManualNative
- **Preconditions**: Two born agents
- **Steps**:
  1. Configure provider preference for agent A.
  2. Switch to agent B and inspect preference.
- **Expected**: No unintended cross-agent state leakage.
- **Evidence**: settings/screens from both identities.

## Exit Criteria

- All P0 spawn lifecycle tests pass.
- Archive/delete guard behavior is deterministic and user-visible.

