# Chunk 02 Implementation Packet (For Approval)

This packet converts approved Identity/Soul Registry decisions into an execution-ready scope.

## Decision Baseline
- Source: `documents/VISION_CHUNK_02_IDENTITY.md`
- Status: Deferred decisions resolved.
- Governance: No implementation until explicit approval of this packet.

## Implementation Scope

### 1) Always-open registry entry and explicit selection
- Target files:
  - `tauri-app/src-ui/src/App.tsx`
  - `tauri-app/src-ui/src/components/SoulRegistry.tsx`
- Planned changes:
  - Ensure app always returns to registry launch flow before active session.
  - Keep explicit mentor selection as required step.

### 2) Archive-first lifecycle UX and open create
- Target files:
  - `tauri-app/src-ui/src/components/SoulRegistry.tsx`
  - `tauri-app/src-ui/src/components/IdentityConflictPanel.tsx`
  - `tauri-app/src/commands/identity.rs`
- Planned changes:
  - Promote archive as primary destructive-safe action.
  - Keep delete as secondary destructive path.
  - Preserve open identity creation path without policy gating.

### 3) Selective sharing contract (skills only)
- Target files:
  - `crates/abigail-core/src/config.rs`
  - `tauri-app/src/commands/identity.rs`
  - `tauri-app/src/commands/config.rs`
  - `tauri-app/src-ui/src/components/ForgePanel.tsx` (or equivalent settings surface)
- Planned changes:
  - Add identity-level sharing settings model.
  - Limit default shareable capability list to skills config/permissions only.
  - Keep all sharing disabled by default until explicit opt-in.

### 4) Soft suspend contract
- Target files:
  - `tauri-app/src-ui/src/App.tsx`
  - `tauri-app/src/commands/identity.rs`
  - optional state surfaces in `tauri-app/src/state.rs`
- Planned changes:
  - Define disconnect as soft suspend preserving chat/context state only.
  - Do not persist pending task queue, subagent run state, or draft inputs by default.

### 5) Migration UX defaults
- Target files:
  - `tauri-app/src-ui/src/components/IdentityConflictPanel.tsx`
  - `tauri-app/src/identity_manager.rs`
- Planned changes:
  - Keep one-click migrate as recommended path.
  - Keep UI simple (no always-visible migration internals).

### 6) Adaptive visual limits
- Target files:
  - `tauri-app/src-ui/src/components/SoulRegistry.tsx`
  - `tauri-app/src/commands/config.rs`
- Planned changes:
  - Allow only minor visual adjustments post-birth.
  - Explicitly disallow full avatar swaps under adaptive mode.

## Out of Scope (Chunk 02)
- Office governance enforcement (deferred to Chunk 4+).
- Broad cross-identity sharing beyond skills.
- Persisting background task/subagent runtime across disconnect.

## Verification Checklist

### Functional
- [ ] Launch enters Soul Registry and requires explicit identity selection.
- [ ] Identity creation remains open and usable.
- [ ] Archive-first action precedence is visible and working.
- [ ] One-click migrate remains primary legacy path.

### State/Policy
- [ ] Soft suspend preserves chat/context only.
- [ ] Sharing defaults are disabled and skills-only when enabled.
- [ ] Adaptive visual changes are constrained to minor adjustments.

### Regression
- [ ] Active identity load/startup check flow remains stable.
- [ ] Birth-complete and chat entry paths remain intact.

## Risks and Rollback
- Risk: Registry entry changes may break active-agent fast paths.
  - Control: keep compatibility branch for already loaded active agent.
  - Rollback: re-enable previous startup shortcut behavior.
- Risk: Sharing model changes introduce config migration complexity.
  - Control: additive config fields with safe defaults.
  - Rollback: ignore new sharing fields and fall back to strict isolation.
- Risk: Soft suspend semantics confuse disconnect expectations.
  - Control: explicit UI copy on disconnect behavior.
  - Rollback: switch to hard disconnect until UX is refined.

## Approval Prompt
- Approve this packet to begin Chunk 02 implementation.
