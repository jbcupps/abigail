# Vision Alignment: Chunk 03 - Chat Experience Contract

## Status
- Phase: Q&A baseline captured, implementation not yet planned.
- Scope: Response style, transparency, tooling visibility, error UX, config surface, memory disclosure, input behavior, and safety posture.

## Captured Decisions (Locked)

### VCC-001: Default response style
- Decision: Balanced style (concise first, depth on demand).

### VCC-002: Routing/provider transparency
- Decision: Minimal by default; reveal details only when needed.

### VCC-003: Tool execution visibility
- Decision: Quiet status badges only.

### VCC-004: Error UX
- Decision: Inline recovery guidance with retry path.

### VCC-005: Config surface
- Decision: Keep provider setup inline in chat.

### VCC-006: Memory disclosure
- Decision: Toggleable disclosure for memory influence.

### VCC-007: Input behavior
- Decision: Enter sends, Shift+Enter newline.

### VCC-008: Safety posture
- Decision: Flexible contextual posture with minimal refusal.

## Deferred Questions
- DQ-01 resolved: auto-show routing details on error, provider/path fallback, and safety-block events.
- DQ-02 resolved: memory disclosure toggle defaults to ON.
- DQ-03 resolved: auto-retry for network timeouts, rate limits, and temporarily unavailable tool backends.
- DQ-04 resolved: flexible safety is bounded by required clarification on ambiguous risky intent and mandatory safer alternatives on refusal.
