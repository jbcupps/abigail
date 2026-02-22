# Skills Test Plan

## Objective

Validate skill invocation reliability, missing-secret handling, error surfacing, and recovery behavior.

## Test Cases

### SKILL-001
- **Title**: Baseline skill invocation (clipboard)
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: Chat available
- **Steps**:
  1. Send clipboard skill prompt.
- **Expected**: Skill-use result is returned and rendered.
- **Evidence**: Response assertion.

### SKILL-002
- **Title**: Missing secret warning flow
- **Priority**: P0
- **Type**: Hybrid
- **Preconditions**: At least one skill requiring secret configured as missing
- **Steps**:
  1. Trigger skill requiring secret.
  2. Observe missing secret indicator.
  3. Open secret modal path.
- **Expected**: Missing secret warning is clear and actionable.
- **Evidence**: UI capture of warning + modal.

### SKILL-003
- **Title**: Injected skill/tool failure handling
- **Priority**: P1
- **Type**: AutomatedHarness
- **Preconditions**: Harness fault controls enabled
- **Steps**:
  1. Inject chat/tool failure mode.
  2. Trigger skill-like prompt.
  3. Clear fault and retry.
- **Expected**: Failure is surfaced; recovery path works after fault reset.
- **Evidence**: test assertion + trace snapshot.

### SKILL-004
- **Title**: Skill creation discoverability
- **Priority**: P1
- **Type**: ManualNative
- **Preconditions**: Operational interface accessible
- **Steps**:
  1. Check Forge/Staff/Registry for explicit create-skill control.
  2. Attempt documented skill-creation path if available.
- **Expected**: Either usable creation flow exists, or gap is explicitly documented.
- **Evidence**: screenshot + capability note.

### SKILL-005
- **Title**: Skill lifecycle smoke (install/enable/disable if exposed)
- **Priority**: P2
- **Type**: ManualNative
- **Preconditions**: UI exposes lifecycle controls
- **Steps**:
  1. Enable/disable or register/unregister selected skill.
  2. Re-run invocation.
- **Expected**: Lifecycle state changes are respected by execution path.
- **Evidence**: before/after behavior capture.

## Exit Criteria

- P0 skill invocation and missing-secret handling pass.
- Any unsupported skill-creation UX is documented with explicit gap status.

