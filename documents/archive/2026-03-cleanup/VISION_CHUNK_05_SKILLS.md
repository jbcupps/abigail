# Vision Alignment: Chunk 05 - Skills and Autonomous Execution

## Status
- Phase: Q&A baseline captured, implementation not yet planned.
- Scope: Autonomy defaults, skill trust/admission, execution visibility, failure recovery, and confirmation boundaries.

## Captured Decisions (Locked)

### VCS-001: Default autonomy
- Decision: High autonomy by default with explicit boundary controls.

### VCS-002: Skill admission policy
- Decision: Trusted auto-approval (manual review for non-trusted).

### VCS-003: Execution visibility
- Decision: Quiet summary with key milestones.

### VCS-004: Failure policy
- Decision: Limited self-recovery before mentor escalation.

### VCS-005: Always-confirm boundaries
- Decision: Require explicit mentor confirmation for:
  - external side effects to new targets,
  - destructive filesystem/data operations,
  - launching long-running persistent autonomous processes.

### VCS-006: Recovery budget
- Decision: Up to 3 retries with strategy variation before escalation.

### VCS-007: Quiet summary contract
- Decision: Summaries must include:
  - intent/goal executed,
  - actions taken,
  - files/state changed.

### VCS-008: Trust source
- Decision: Signed skill allowlist is the primary trust source for auto-approval.

## Deferred Questions
- DQ-01 resolved: "new target" is defined as new recipient (not domain) for external side-effect confirmations.
- DQ-02 resolved: allowed retry strategy variations are parameter tuning, provider/model swap, tool fallback, timing/backoff adjustment, and scope reduction.
- DQ-03 resolved: quiet summaries must include confidence/uncertainty line.
- DQ-04 resolved: allowlist operations use hybrid model (manual baseline + helper commands).
