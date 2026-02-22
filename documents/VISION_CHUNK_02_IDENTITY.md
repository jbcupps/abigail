# Vision Alignment: Chunk 02 - Identity and Soul Registry

## Status
- Phase: Decisions approved for clean-restart implementation (2026-02-22).
- Scope: Identity selection, creation, migration posture, isolation, lifecycle handling, and immediate runtime limits.
- Terminology standard: `Hive > Entity > Agent`.

## Captured Decisions (Locked)

### VCI-001: Registry entry behavior
- Decision: Always show Soul Registry at launch; explicit mentor selection each session.

### VCI-002: Creation policy
- Decision: Open entity creation anytime for mentor.

### VCI-003: Archive vs delete UX
- Decision: Archive-first primary action; delete remains secondary/destructive.

### VCI-004: Isolation model
- Decision: Cross-entity sharing is off by default.
- Decision: If enabled, sharing scope can include skills, memory, and tools with explicit opt-in.

### VCI-005: Office governance timing
- Decision: Defer strict office governance mechanics to Chunk 4+.

### VCI-006: Visual profile control
- Decision: Full avatar/theme swaps are allowed post-birth.

### VCI-007: Migration posture
- Decision: Treat migration as low-priority in current clean-restart phase.
- Decision: Auto-migrate silently when migration is needed later.
- Decision: Conflict behavior is prompt-per-conflict with hooks for future risk scoring.

### VCI-008: Disconnect behavior
- Decision: Soft suspend/resumable context as default session disconnect behavior.
- Decision: Preserve chat/context only by default.

### VCI-009: Provider scope and ownership
- Decision: Provider configuration is Hive-global by default.
- Decision: Support per-entity provider overrides for task optimization.

### VCI-010: Runtime controls
- Decision: Cap concurrently running agents at 3 for now.
- Decision: Add hooks for future dynamic caps based on usage and hardware.
- Decision: Add scrolling support in registry/management surfaces where needed.

### VCI-011: Authority model
- Decision: Entity autonomy is allowed within policy bounds (mentor-defined constraints still apply).
