# Operational Hive UI Test Plan

## Objective

Validate day-2 operation of the Hive interface: chat stability, panel navigation, routing telemetry, and session continuity.

## Test Cases

### OPER-001
- **Title**: Chat turn baseline stability
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: App in chat state
- **Steps**:
  1. Send normal prompt.
  2. Wait for assistant response completion.
- **Expected**: Response renders without lockup/crash.
- **Evidence**: Response assertion and no runtime error.

### OPER-002
- **Title**: Clipboard skill-use turn
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: App in chat state
- **Steps**:
  1. Send clipboard-intent prompt.
- **Expected**: Tool status and clipboard result response appear.
- **Evidence**: Result text assertion and status event trace.

### OPER-003
- **Title**: Routing detail telemetry visibility
- **Priority**: P1
- **Type**: Hybrid
- **Preconditions**: Chat state, provider configured
- **Steps**:
  1. Toggle routing details visibility.
  2. Send prompt.
- **Expected**: Provider/routing detail visible and consistent with active provider.
- **Evidence**: UI capture + trace/reference text.

### OPER-004
- **Title**: Sanctum/Forge panel navigation resilience
- **Priority**: P1
- **Type**: ManualNative
- **Preconditions**: Chat loaded
- **Steps**:
  1. Open/close Sanctum.
  2. Navigate Forge/Staff/Registry tabs.
  3. Return to chat and send prompt.
- **Expected**: No blank-screen crash; chat remains usable.
- **Evidence**: Screenshot/video + note.

### OPER-005
- **Title**: Session continuity on disconnect/reconnect
- **Priority**: P2
- **Type**: Hybrid
- **Preconditions**: Existing active identity
- **Steps**:
  1. Send 1-2 messages.
  2. Disconnect to management.
  3. Reload same identity.
- **Expected**: Session behavior matches expected snapshot policy.
- **Evidence**: Before/after message state capture.

## Exit Criteria

- All P0 operational cases pass.
- No reproducible crash during panel navigation.

