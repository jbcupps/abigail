# Crystallization UI Test Plan

## Objective

Validate identity formation flows, input validation, persistence, and transition to emergence.

## Test Cases

### CRYS-001
- **Title**: Path selector baseline
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: Reached Genesis path selector
- **Steps**:
  1. Verify options are visible.
  2. Select `Fast Template`.
  3. Click `Begin`.
- **Expected**: Crystallization fast-template path loads.
- **Evidence**: Path header and template controls assertion.

### CRYS-002
- **Title**: Fast template completion
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: Fast template path loaded
- **Steps**:
  1. Select template.
  2. Click `Use Template`.
- **Expected**: Soul preview form appears with prefilled values.
- **Evidence**: Form values and controls visible.

### CRYS-003
- **Title**: Required name validation
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: Soul preview stage
- **Steps**:
  1. Clear Agent Name.
  2. Click `Crystallize Soul`.
- **Expected**: Validation error shown and no transition.
- **Evidence**: Error text and stage unchanged.

### CRYS-004
- **Title**: Crystallize and transition to emergence
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: Valid soul preview fields
- **Steps**:
  1. Enter valid agent details.
  2. Click `Crystallize Soul`.
- **Expected**: Emergence stage appears with ceremony action.
- **Evidence**: Emergence heading/button assertion.

### CRYS-005
- **Title**: Visual identity control persistence
- **Priority**: P1
- **Type**: Hybrid
- **Preconditions**: Soul preview stage
- **Steps**:
  1. Set color and avatar fields.
  2. Crystallize.
  3. Re-open relevant identity displays post-birth.
- **Expected**: Configured identity visuals persist.
- **Evidence**: UI capture before/after.

### CRYS-006
- **Title**: Alternative path smoke coverage
- **Priority**: P2
- **Type**: ManualNative
- **Preconditions**: Genesis selector
- **Steps**:
  1. Execute one non-fast path (`guided_dialog` or `direct`).
  2. Complete to emergence.
- **Expected**: Alternative path remains functional and converges.
- **Evidence**: Screenshot and notes.

## Exit Criteria

- All P0 crystallization cases pass.
- At least one non-fast path verified in current release cycle.

