# Chunk 01 Implementation Packet (For Approval)

This packet translates approved Birth decisions into an execution-ready, gated plan.

## Decision Baseline
- Source: `documents/VISION_CHUNK_01_BIRTH.md`
- Status: Deferred questions resolved.
- Governance: No implementation until explicit approval of this packet.

## Implementation Scope

### 1) Birth tone and stage contract
- Target files:
  - `tauri-app/src-ui/src/components/BootSequence.tsx`
- Planned changes:
  - Establish stage-tone map:
    - `Darkness`, `KeyPresentation`, `Ignition`, `Connectivity`: clear/professional copy.
    - `Genesis*`, `Crystallization`, `SoulPreview`: guided reflective copy.
    - `Emergence`, `Life`: cinematic ceremonial copy and pacing.
  - Ensure copy is consistent with trust and setup intent per stage.

### 2) Provider-required connectivity gate
- Target files:
  - `tauri-app/src-ui/src/components/BootSequence.tsx`
  - `tauri-app/src/commands/config.rs`
  - `tauri-app/src/lib.rs`
- Planned changes:
  - Keep UI gate requiring at least one configured provider.
  - Add backend guard so progression cannot bypass the gate via UI-only path.
  - Ensure clear error message when transition is attempted without provider.

### 3) Crystallization path modularization (initial five-path set)
- Target files:
  - `tauri-app/src-ui/src/components/GenesisPathSelector.tsx`
  - `tauri-app/src-ui/src/components/BootSequence.tsx`
  - New components:
    - `tauri-app/src-ui/src/components/CrystallizationPathFast.tsx`
    - `tauri-app/src-ui/src/components/CrystallizationPathDialog.tsx`
    - `tauri-app/src-ui/src/components/CrystallizationPathImage.tsx`
    - `tauri-app/src-ui/src/components/CrystallizationPathPsychQuestions.tsx`
    - `tauri-app/src-ui/src/components/CrystallizationPathTemplateEdit.tsx`
  - Optional path registry helper:
    - `tauri-app/src-ui/src/components/crystallizationPaths.ts`
- Planned changes:
  - Add modular path registration model.
  - Implement five initial paths:
    - Fast template pick.
    - Dialog progressive disclosure.
    - Image-choice (2-4 options per panel, up to 10 panels; bundled assets first).
    - Psychological question path with moral-choice dimension.
    - Editable template path.
  - Route outputs through shared interface.

### 4) Soul/personality output boundaries
- Target files:
  - `tauri-app/src/commands/identity.rs`
  - `tauri-app/src/lib.rs`
  - `crates/abigail-core/src/config.rs` (or adjacent identity persistence surfaces)
- Planned changes:
  - Explicitly separate immutable soul payload from adaptive personality payload.
  - Lock soul persistence semantics.
  - Add personality metadata with daily adaptation cadence policy hook.

### 5) Repair behavior parity
- Target files:
  - `tauri-app/src-ui/src/components/BootSequence.tsx`
- Planned changes:
  - Preserve balanced prominence between recovery and reset.
  - Tighten explanatory copy and consequences for both options.

## Out of Scope (Chunk 01)
- Office policy templates/admin locks (deferred to Chunk 4+).
- Dynamic generated image pipelines for image path (bundled-first policy in Chunk 01).
- Deep personality adaptation engine implementation (only boundary/cadence contract in this chunk).

## Verification Checklist

### Functional
- [ ] Birth flow enforces provider requirement before advancing.
- [ ] All five crystallization paths are selectable and complete successfully.
- [ ] Each path returns valid output with both soul and personality fields.
- [ ] Repair options remain available and functional.

### UX
- [ ] Stage copy follows approved tone progression.
- [ ] Key handling still supports auto-save recommended path and manual copy.
- [ ] Emergence sequence presents cinematic completion pacing.

### Policy and Data
- [ ] Soul output is persisted as immutable contract surface.
- [ ] Personality output is persisted as adaptive surface with daily cadence marker.

### Regression
- [ ] Existing first-run path still completes under target duration (3-7 minutes).
- [ ] Startup and post-birth transition to chat remains intact.

## Risk Controls and Rollback
- Risk: Path modularization destabilizes current Birth.
  - Control: Keep legacy path behind fallback switch during migration.
  - Rollback: Revert to current direct path selector and single flow components.
- Risk: Backend provider gate conflicts with existing skip paths.
  - Control: Add explicit policy check and user-facing guidance text.
  - Rollback: Temporarily relax backend gate while preserving UI warning.
- Risk: New payload boundaries break existing load/migration.
  - Control: Backward-compatible schema read with defaults.
  - Rollback: Compatibility shim to old combined payload model.

## Approval Prompt
- Approve this packet to begin Chunk 01 implementation.
