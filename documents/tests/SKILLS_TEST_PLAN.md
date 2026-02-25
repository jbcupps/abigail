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

### SKILL-006
- **Title**: Email credential setup via LLM tool-use
- **Priority**: P0
- **Type**: AutomatedHarness
- **Preconditions**: `ABIGAIL_IMAP_TEST=1`, cloud LLM API key set, entity-chat crate
- **Steps**:
  1. Send message with IMAP/SMTP connection details.
  2. LLM recognizes credential-setup intent.
  3. LLM calls `store_secret` for each credential via tool-use loop.
- **Expected**: All credentials stored in vault; `store_secret` calls succeed.
- **Evidence**: `cargo test -p entity-chat --test live_email turn1_credential_setup`

### SKILL-007
- **Title**: Email fetch via IMAP skill
- **Priority**: P1
- **Type**: AutomatedHarness
- **Preconditions**: `ABIGAIL_IMAP_TEST=1`, cloud LLM API key, Proton Mail Bridge running
- **Steps**:
  1. Pre-populate vault with IMAP credentials.
  2. Initialize ProtonMailSkill (IMAP connection).
  3. Send message asking to check email from a specific sender.
  4. LLM calls `fetch_emails` tool.
- **Expected**: `fetch_emails` invoked; response references the target sender.
- **Evidence**: `cargo test -p entity-chat --test live_email turn2_fetch_emails`

### SKILL-008
- **Title**: Email send via SMTP skill
- **Priority**: P1
- **Type**: AutomatedHarness
- **Preconditions**: `ABIGAIL_IMAP_TEST=1`, cloud LLM API key, Proton Mail Bridge running
- **Steps**:
  1. Pre-populate vault with IMAP/SMTP credentials.
  2. Initialize ProtonMailSkill (IMAP + SMTP connection).
  3. Send message asking to email a specific address.
  4. LLM calls `send_email` tool.
- **Expected**: `send_email` invoked and succeeds.
- **Evidence**: `cargo test -p entity-chat --test live_email turn3_send_email`

## Exit Criteria

- P0 skill invocation and missing-secret handling pass.
- Any unsupported skill-creation UX is documented with explicit gap status.

