# Birth UI Test Plan

## Objective

Validate first-run onboarding from empty registry through successful transition to chat readiness.

## Test Cases

### BIRTH-001
- **Title**: Empty state to birth entry
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: No entities present
- **Steps**:
  1. Open app.
  2. Skip splash.
  3. Confirm empty-state registry and enter entity name.
  4. Click `Birth`.
- **Expected**: Transition to key presentation.
- **Evidence**: Assertion for key presentation title.

### BIRTH-002
- **Title**: Signing key acknowledgement gate
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: On key presentation stage
- **Steps**:
  1. Verify `Continue` disabled before checkbox.
  2. Check acknowledgement box.
  3. Click `Continue`.
- **Expected**: Transition to Ignition stage.
- **Evidence**: Disabled/enabled state + stage assertion.

### BIRTH-003
- **Title**: Ignition deterministic path
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: On Ignition stage
- **Steps**:
  1. Click `Continue without model`.
- **Expected**: Connectivity command center appears.
- **Evidence**: Connectivity header assertion.

### BIRTH-004
- **Title**: Birth completion to chat
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: Connectivity stage with provider(s) configured
- **Steps**:
  1. Establish linkage.
  2. Complete crystallization path.
  3. Complete emergence ceremony.
- **Expected**: Main chat window is reachable.
- **Evidence**: Chat message input assertion.

### BIRTH-005
- **Title**: Provider requirement blocking behavior
- **Priority**: P1
- **Type**: Hybrid
- **Preconditions**: Connectivity stage with zero providers
- **Steps**:
  1. Attempt to continue to crystallization.
- **Expected**: Blocking message shown; no transition.
- **Evidence**: Error text + stage remains Connectivity.

### BIRTH-006
- **Title**: Recovery path smoke
- **Priority**: P2
- **Type**: ManualNative
- **Preconditions**: Force broken/interrupted birth scenario
- **Steps**:
  1. Trigger repair/reset path.
  2. Verify retry behavior.
- **Expected**: App offers clear recovery action and re-enters valid flow.
- **Evidence**: Screenshot + notes.

## Exit Criteria

- All P0 birth cases pass.
- No unresolved P0 blocker in onboarding stages.

