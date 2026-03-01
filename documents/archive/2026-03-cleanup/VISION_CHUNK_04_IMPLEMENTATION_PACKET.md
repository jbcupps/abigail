# Chunk 04 Implementation Packet (For Approval)

This packet translates approved Forge UX decisions into an execution-ready scope.

## Decision Baseline
- Source: `documents/VISION_CHUNK_04_FORGE.md`
- Status: Deferred decisions resolved.
- Governance: No implementation until explicit approval of this packet.

## Implementation Scope

### 1) Dual-view Forge navigation
- Target files:
  - `tauri-app/src-ui/src/components/ForgePanel.tsx`
  - adjacent Forge subcomponents/tabs
- Planned changes:
  - Introduce switchable view model:
    - Task view (Diagnose, Configure, Build, Audit)
    - System view (Router, Skills, Memory, Identity, Ops)
  - Preserve current functionality while restructuring navigation only.

### 2) Adaptive complexity via explicit toggle
- Target files:
  - `tauri-app/src-ui/src/components/ForgePanel.tsx`
  - `tauri-app/src/commands/config.rs`
  - `crates/abigail-core/src/config.rs` (if persisted)
- Planned changes:
  - Add explicit complexity toggle (Basic/Advanced).
  - Default to adaptive entry behavior but transition only by user toggle.

### 3) High-risk confirm gate
- Target files:
  - `tauri-app/src-ui/src/components/ForgePanel.tsx`
  - `tauri-app/src/commands/identity.rs`
  - `tauri-app/src/commands/config.rs`
- Planned changes:
  - Require confirmation for:
    - destructive identity operations,
    - security/policy setting changes.
  - Keep non-high-risk actions fast path.

### 4) Preview + undo workflow
- Target files:
  - `tauri-app/src-ui/src/components/ForgePanel.tsx`
  - `tauri-app/src/commands/forge.rs` (or equivalent command surface)
  - optional operation log helper in backend state
- Planned changes:
  - Add preview-before-apply for impactful changes.
  - Add multi-step undo timeline with 30-minute retention window.

### 5) Toggleable audit panel minimum contract
- Target files:
  - `tauri-app/src-ui/src/components/ForgePanel.tsx`
  - `tauri-app/src/commands/forge.rs` / logging surfaces
- Planned changes:
  - Audit panel toggle control.
  - Minimum event fields shown when enabled:
    - actor + timestamp,
    - what changed,
    - risk level,
    - execution outcome.

### 6) Guardrailed persona controls
- Target files:
  - `tauri-app/src-ui/src/components/ForgePanel.tsx`
  - `tauri-app/src/commands/config.rs`
- Planned changes:
  - Preset/slider-based behavior tuning with constrained ranges.
  - No direct low-level document editing in default Forge flow.

## Out of Scope (Chunk 04)
- Office governance policy packs implementation in this repo (explicitly deferred to Orion Dock integration path).
- Full policy engine replacement.
- Major backend architecture refactor unrelated to Forge UX.

## Verification Checklist

### Functional
- [ ] Forge supports task/system dual navigation with no feature loss.
- [ ] Complexity mode changes only via explicit toggle.
- [ ] High-risk action confirmations trigger for destructive identity and policy/security changes.
- [ ] Preview + undo works with multi-step 30-minute history.
- [ ] Audit panel toggles and shows required minimum fields.

### UX
- [ ] Basic mode remains clean and concise for daily use.
- [ ] Advanced mode exposes deeper controls without breaking task flow.
- [ ] Persona sliders enforce guardrails and clear value boundaries.

### Regression
- [ ] Existing Forge actions continue to execute correctly.
- [ ] No unintended confirmation prompts on low-risk operations.
- [ ] Existing chat/sanctum integration remains stable.

## Risks and Rollback
- Risk: Dual-view navigation increases cognitive load.
  - Control: default to a clear primary view with preserved task labels.
  - Rollback: keep one view and expose the other as optional advanced panel.
- Risk: Undo history introduces state complexity.
  - Control: operation-scoped reversible actions first, then expand.
  - Rollback: keep preview-only mode while undo backend matures.
- Risk: High-risk classifier misses operations.
  - Control: maintain explicit operation allowlist for initial release.
  - Rollback: temporarily broaden confirm scope.

## Approval Prompt
- Approve this packet to begin Chunk 04 implementation.
