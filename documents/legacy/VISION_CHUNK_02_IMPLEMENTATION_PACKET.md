# Chunk 02 Implementation Packet (Approved Baseline)

This packet converts approved Identity/Soul Registry decisions into an execution-ready scope.

## Decision Baseline
- Source: `documents/VISION_CHUNK_02_IDENTITY.md`
- Status: Approved baseline captured on 2026-02-22.
- Governance: Proceed with implementation on current branch using clean-restart assumptions.

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

### 3) Selective sharing contract (skills + memory + tools)
- Target files:
  - `crates/abigail-core/src/config.rs`
  - `tauri-app/src/commands/identity.rs`
  - `tauri-app/src/commands/config.rs`
  - `tauri-app/src-ui/src/components/ForgePanel.tsx` (or equivalent settings surface)
- Planned changes:
  - Add identity-level sharing settings model.
  - Keep all sharing disabled by default until explicit opt-in.
  - Allow selectable sharing domains across skills, memory, and tools.

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
  - Treat migration as low-priority in the current clean-restart phase.
  - Implement silent auto-migration for supported schema/config updates.
  - If conflicts occur, prompt per conflict and route through a future risk-scoring hook.

### 6) Visual adaptation behavior
- Target files:
  - `tauri-app/src-ui/src/components/SoulRegistry.tsx`
  - `tauri-app/src/commands/config.rs`
- Planned changes:
  - Allow full avatar/theme swaps post-birth.
  - Preserve guardrails only for destructive/non-visual policy surfaces.

### 7) Runtime cap and scaling hooks
- Target files:
  - `tauri-app/src/state.rs`
  - `tauri-app/src/commands/identity.rs`
  - `tauri-app/src-ui/src/components/SoulRegistry.tsx`
- Planned changes:
  - Enforce maximum 3 concurrently running agents for now.
  - Add abstraction hooks so runtime cap can be derived from usage and hardware in a later pass.
  - Ensure registry and lifecycle surfaces support scrolling when lists/actions overflow.

### 8) Provider ownership model
- Target files:
  - `tauri-app/src/commands/config.rs`
  - `crates/abigail-core/src/config.rs`
  - `tauri-app/src-ui/src/components/ForgePanel.tsx` (or equivalent settings surface)
- Planned changes:
  - Keep provider configuration Hive-global by default.
  - Add per-entity override path focused on task optimization.

## Out of Scope (Chunk 02)
- Office governance enforcement (deferred to Chunk 4+).
- Persisting background task/subagent runtime across disconnect.
- In-place compatibility for unused legacy release-updater flows (clean restart policy active for this phase).

## Verification Checklist

### Functional
- [ ] Launch enters Soul Registry and requires explicit identity selection.
- [ ] Identity creation remains open and usable.
- [ ] Archive-first action precedence is visible and working.
- [ ] Scrolling behavior is correct for long entity/agent lists.
- [ ] Running-agent cap of 3 is enforced.

### State/Policy
- [ ] Soft suspend preserves chat/context only.
- [ ] Sharing defaults are disabled and selectable across skills/memory/tools when enabled.
- [ ] Full avatar/theme swaps are supported post-birth.
- [ ] Provider defaults are Hive-global, with per-entity override support.

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
- Risk: Fixed cap of 3 running agents may block valid heavy workloads.
  - Control: expose cap in one location and add hardware/usage-driven hook interface.
  - Rollback: temporarily raise static cap by config while dynamic policy is implemented.

## Execution Prompt
- Implement this packet as the canonical path for current branch alignment.
