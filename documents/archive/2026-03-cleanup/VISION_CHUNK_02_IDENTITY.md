# Vision Alignment: Chunk 02 - Identity and Soul Registry

## Status
- Phase: Q&A baseline captured, implementation not yet planned.
- Scope: Identity selection, creation, migration, isolation, and lifecycle handling.

## Captured Decisions (Locked)

### VCI-001: Registry entry behavior
- Decision: Always show Soul Registry at launch; explicit mentor selection each session.

### VCI-002: Creation policy
- Decision: Open identity creation anytime for mentor.

### VCI-003: Archive vs delete UX
- Decision: Archive-first primary action; delete remains secondary/destructive.

### VCI-004: Isolation model
- Decision: Selective sharing model with explicit opt-in per capability.

### VCI-005: Office governance timing
- Decision: Defer strict office governance mechanics to Chunk 4+.

### VCI-006: Visual profile control
- Decision: Adaptive visuals allowed, but constrained by mentor-defined limits.

### VCI-007: Legacy migration default
- Decision: One-click migrate recommended path.

### VCI-008: Disconnect behavior
- Decision: Soft suspend/resumable context as default session disconnect behavior.

## Deferred Questions
- DQ-01 resolved: only skill permissions/config are selectable for cross-identity sharing by default.
- DQ-02 resolved: adaptive visuals limited to minor adjustments only (no full avatar swap).
- DQ-03 resolved: soft suspend persists chat/context state only.
- DQ-04 resolved: migration UI is simple button only (technical details hidden).
