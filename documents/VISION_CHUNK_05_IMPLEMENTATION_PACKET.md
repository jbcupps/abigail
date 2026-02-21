# Chunk 05 Implementation Packet (For Approval)

This packet translates approved Skills/Autonomy decisions into an implementation-ready scope.

## Decision Baseline
- Source: `documents/VISION_CHUNK_05_SKILLS.md`
- Status: Deferred decisions resolved.
- Governance: No implementation until explicit approval of this packet.

## Implementation Scope

### 1) High-autonomy execution mode with hard boundaries
- Target files:
  - `tauri-app/src/commands/chat.rs`
  - `tauri-app/src/commands/skills.rs`
  - `crates/abigail-skills/src/executor.rs` (or adjacent execution guards)
- Planned changes:
  - Maintain high-autonomy default path.
  - Enforce mandatory confirmation boundaries for:
    - external side effects to new recipients,
    - destructive filesystem/data operations,
    - long-running autonomous process launches.

### 2) Trusted auto-approval via signed allowlist
- Target files:
  - `crates/abigail-core/src/config.rs`
  - `tauri-app/src/commands/skills.rs`
  - optional helper in `tauri-app/src/commands/config.rs`
- Planned changes:
  - Use signed allowlist as primary trust source for auto-approval.
  - Keep manual baseline controls.
  - Add helper commands for allowlist rotate/revoke while preserving manual fallback.

### 3) Recovery budget and strategy-variation policy
- Target files:
  - `tauri-app/src/commands/chat.rs`
  - `crates/abigail-router/src/router.rs` (if provider-switch logic is needed)
  - `crates/abigail-skills/src/executor.rs`
- Planned changes:
  - Retry budget: max 3 attempts.
  - Allowed strategy variation set:
    - parameter tuning,
    - provider/model swap,
    - tool fallback/substitution,
    - timing/backoff,
    - scope reduction.

### 4) Quiet summary contract with confidence line
- Target files:
  - `tauri-app/src-ui/src/components/ChatInterface.tsx`
  - `tauri-app/src/commands/chat.rs`
- Planned changes:
  - Keep quiet-summary UX.
  - Always include:
    - intent/goal,
    - actions taken,
    - files/state changed,
    - confidence/uncertainty statement.

### 5) New-recipient confirmation policy
- Target files:
  - `tauri-app/src/commands/chat.rs`
  - skill-side recipient handling surfaces (email/notification/integration skills)
- Planned changes:
  - Define and track known recipients per identity context.
  - Require explicit confirmation when recipient is first-time/new.

## Out of Scope (Chunk 05)
- Full enterprise policy orchestration and central trust management.
- Replacing existing skill runtime architecture.
- Broad UI redesign outside summary and confirmation surfaces.

## Verification Checklist

### Functional
- [ ] High-autonomy path remains default for low-risk operations.
- [ ] New-recipient side effects require confirmation.
- [ ] Destructive filesystem/data operations require confirmation.
- [ ] Long-running autonomous launches require confirmation.
- [ ] Auto-approval works for signed allowlisted skills.

### Recovery
- [ ] Retry cap enforced at 3 attempts.
- [ ] Strategy variation progression is visible/logged per attempt.
- [ ] Escalation to mentor occurs after budget exhaustion.

### Summary/Audit
- [ ] Quiet summaries include intent/actions/changes/confidence.
- [ ] Confidence line reflects uncertainty clearly.

### Regression
- [ ] Existing approved skill executions still run.
- [ ] Normal chat flow remains responsive.
- [ ] Existing confirmation pathways remain stable.

## Risks and Rollback
- Risk: Over-confirmation harms autonomy UX.
  - Control: confirm only policy-bound categories.
  - Rollback: temporarily narrow confirm categories to destructive only.
- Risk: Retry strategy variation becomes opaque.
  - Control: emit concise per-attempt strategy tags in summaries/audit.
  - Rollback: reduce to deterministic retry policy.
- Risk: Allowlist helper commands introduce trust drift.
  - Control: require explicit signed source metadata on helper updates.
  - Rollback: revert to manual-only allowlist updates.

## Approval Prompt
- Approve this packet to begin Chunk 05 implementation.
