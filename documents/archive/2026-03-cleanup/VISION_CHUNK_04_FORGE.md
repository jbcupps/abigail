# Vision Alignment: Chunk 04 - Forge and Advanced Tooling UX

## Status
- Phase: Q&A baseline captured, implementation not yet planned.
- Scope: Forge purpose, navigation, safety workflows, audit visibility, and behavior controls.

## Captured Decisions (Locked)

### VCF-001: Forge primary role
- Decision: Hybrid workspace (control + creation).

### VCF-002: Default complexity
- Decision: Adaptive default complexity by user profile/context.

### VCF-003: Tool execution gating
- Decision: Confirm high-risk operations only.

### VCF-004: Navigation model
- Decision: Dual-view navigation (task-oriented and system-oriented switch).

### VCF-005: Office governance in this repo
- Decision: Defer office governance implementation here and reference Orion Dock patterns externally.

### VCF-006: Change safety workflow
- Decision: Both preview-impact and undo pathways are required.

### VCF-007: Audit visibility
- Decision: Toggleable audit panel.

### VCF-008: Persona/behavior controls
- Decision: Guardrailed sliders/presets (not direct low-level editing by default).

## Deferred Questions
- DQ-01 resolved: adaptive complexity transitions are controlled by explicit user toggle.
- DQ-02 resolved: high-risk confirmations are required for destructive identity actions and security/policy changes.
- DQ-03 resolved: undo contract is multi-step history with 30-minute window.
- DQ-04 resolved: audit panel minimums are actor/timestamp, what changed, risk level, and execution outcome.
