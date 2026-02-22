# Chunk 03 Implementation Packet (For Approval)

This packet translates approved Chat Experience decisions into an execution-ready scope.

## Decision Baseline
- Source: `documents/VISION_CHUNK_03_CHAT.md`
- Status: Deferred decisions resolved.
- Governance: No implementation until explicit approval of this packet.

## Implementation Scope

### 1) Balanced response defaults
- Target files:
  - `tauri-app/src/commands/chat.rs`
  - `tauri-app/src-ui/src/components/ChatInterface.tsx`
- Planned changes:
  - Bias assistant output to concise-first responses with optional expansion hooks.
  - Preserve existing streaming and status behavior.

### 2) Minimal transparency with explicit auto-reveal triggers
- Target files:
  - `tauri-app/src-ui/src/components/ChatInterface.tsx`
  - `tauri-app/src/commands/chat.rs`
- Planned changes:
  - Keep provider/routing details hidden by default.
  - Auto-reveal details when:
    - error occurs,
    - fallback path/provider is used,
    - safety block/refusal event occurs.

### 3) Quiet tool status + inline recovery errors
- Target files:
  - `tauri-app/src-ui/src/components/ChatInterface.tsx`
  - `tauri-app/src/commands/chat.rs`
- Planned changes:
  - Keep tool feedback as lightweight status badges.
  - Standardize inline recovery guidance and one-click retry affordances.

### 4) Retry policy contract
- Target files:
  - `tauri-app/src/commands/chat.rs`
  - optional helper in `tauri-app/src/rate_limit.rs` or adjacent retry utility
- Planned changes:
  - Auto-retry only:
    - network timeout failures,
    - rate-limit failures,
    - temporary tool-backend unavailability.
  - Do not auto-retry non-transient or policy failures.

### 5) Memory disclosure toggle defaults
- Target files:
  - `tauri-app/src-ui/src/components/ChatInterface.tsx`
  - `tauri-app/src/commands/config.rs`
  - `crates/abigail-core/src/config.rs` (if persisted in config)
- Planned changes:
  - Add user-toggle memory disclosure control with default ON.
  - Scope preference per identity/session based on existing config model.

### 6) Flexible-contextual safety guardrails
- Target files:
  - `tauri-app/src/commands/chat.rs`
  - `crates/abigail-router/src/router.rs` (if routing-layer signals are required)
- Planned changes:
  - On ambiguous risky intent: require clarifying question.
  - On refusal: require safer alternative guidance in response payload.

## Out of Scope (Chunk 03)
- Moving chat configuration to a separate settings panel (kept inline by decision).
- Full transcript-style tool logs.
- Major routing architecture changes beyond visibility/retry/signaling.

## Verification Checklist

### Functional
- [ ] Balanced response behavior visible in normal prompts.
- [ ] Transparency stays minimal by default.
- [ ] Transparency auto-reveals on error, fallback, and safety-block events.
- [ ] Memory disclosure toggle exists and defaults ON.
- [ ] Inline retry appears for eligible transient failures.

### Safety/Policy
- [ ] Ambiguous risky requests trigger clarification, not immediate completion.
- [ ] Refusal responses include safer alternatives.
- [ ] Non-transient failures do not auto-retry.

### Regression
- [ ] Streaming remains stable.
- [ ] Existing provider setup in chat remains usable.
- [ ] Tool status events still display without noisy transcript output.

## Risks and Rollback
- Risk: Retry logic can duplicate side effects on non-idempotent tools.
  - Control: restrict auto-retry to explicitly transient classes only.
  - Rollback: disable auto-retry globally via guard flag.
- Risk: Overly hidden routing data can reduce debuggability.
  - Control: clear reveal triggers and optional user-initiated reveal action.
  - Rollback: revert to always-visible status line temporarily.
- Risk: Safety clarifications may add friction in benign cases.
  - Control: apply only to ambiguous high-risk intent patterns.
  - Rollback: narrow trigger thresholds.

## Approval Prompt
- Approve this packet to begin Chunk 03 implementation.
