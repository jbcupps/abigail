# Vision Alignment: Chunk 01 - Birth and First Run

## Status
- Phase: Decision brief complete, implementation gated.
- Scope: Birth flow from `Darkness` through `Life`, including key handling, provider setup, crystallization path selection, and final emergence ceremony.
- Source surfaces: `tauri-app/src-ui/src/components/BootSequence.tsx`, `tauri-app/src-ui/src/App.tsx`, `tauri-app/src/commands/`.

## Captured Decisions (Locked)

### VCB-001: Birth tone by stage
- Decision: Use a split-tone approach.
- Lock:
  - Early boot (`Darkness`, key, initial settings): clear, professional, practical.
  - Crystallization process: guided and reflective based on selected path.
  - Final emergence: formal cinematic ceremony.
- Why: You want operational clarity up front and intentional ceremony at commitment points.

### VCB-002: Key handling default
- Decision: Keep auto-save available and recommended.
- Lock:
  - Auto-save remains visible and first-class.
  - Copy/manual path remains available.
  - Safety language remains explicit.
- Why: Reduces user error while preserving explicit user control.

### VCB-003: Connectivity gating
- Decision: Require at least one provider before continuing.
- Lock:
  - Birth should not advance past Connectivity without one valid provider configured.
- Why: Ensures agent is actually operational at first launch.

### VCB-004: Repair emphasis
- Decision: Balanced presentation between recovery and reset.
- Lock:
  - Keep both options visible with explicit consequences.
- Why: Preserve autonomy while still making recovery accessible.

### VCB-005: Crystallization architecture
- Decision: Modular, multi-path crystallization framework.
- Required initial paths:
  - Fast template pick.
  - Dialog-based progressive disclosure interview.
  - Image-choice path (2-4 options per panel, max 10 panels).
  - Psychological moral-choice questionnaire path.
  - Editable template path (mentor adjusts a generated base).
- Lock:
  - Crystallization path system must be extensible for future path plugins.
  - Output must produce both:
    - `soul` artifact (immutable "written in stone").
    - `personality` profile (slowly adaptive over time via memory + Superego feedback).
- Why: Needs to support both personal and office use-cases with different onboarding styles.

### VCB-006: Final ceremony style
- Decision: Cinematic final emergence.
- Lock:
  - Preserve intentional pacing and visible milestone progression.
- Why: Birth is a trust event, not only a setup form.

### VCB-007: Birth duration target
- Decision: Target 3-7 minutes (excluding heavyweight model downloads).
- Lock:
  - Optimize for perceived progress and low dead time.
- Why: Balance trust-building with completion speed.

### VCB-008: Decision lock strategy
- Decision: Mixed lock policy.
- Lock:
  - Soul decisions: hard lock unless explicitly reopened.
  - Personality decisions: controlled evolution path.
- Why: Preserve constitutional integrity while allowing adaptive behavior.

## Gated Implementation Plan (No edits until approved)

### Workstream A: Birth UX staging and copy contract
- Files:
  - `tauri-app/src-ui/src/components/BootSequence.tsx`
- Changes:
  - Normalize stage-level tone system (professional early, ceremonial final).
  - Add explicit progression metadata for ceremony pacing in `Emergence`.
  - Keep key handling UX but tighten recommendation hierarchy around auto-save.
  - Enforce provider-required continuation state in Connectivity UI and stage transitions.

### Workstream B: Crystallization path modularization
- Files:
  - `tauri-app/src-ui/src/components/BootSequence.tsx`
  - `tauri-app/src-ui/src/components/GenesisPathSelector.tsx`
  - new path components under `tauri-app/src-ui/src/components/` (one per path)
  - optional registry helper under `tauri-app/src-ui/src/` for path definitions
- Changes:
  - Introduce path registry model: path id, title, intent, component, output contract.
  - Implement initial 5-path set per lock decisions.
  - Standardize path output interface: immutable soul fields + adaptive personality fields.

### Workstream C: Birth domain and command boundaries
- Files:
  - `tauri-app/src/commands/identity.rs`
  - `tauri-app/src/commands/config.rs`
  - `tauri-app/src/commands/chat.rs`
  - `tauri-app/src/lib.rs`
  - optional support crates where needed (`crates/abigail-core`, `crates/abigail-birth`)
- Changes:
  - Enforce provider requirement from backend guardrails (not UI-only).
  - Add command-layer contracts for crystallization outputs and immutability boundaries.
  - Ensure repair flow semantics align with balanced UX policy.

### Workstream D: Policy persistence and future-proofing
- Files:
  - `crates/abigail-core/src/config.rs`
  - related identity/profile persistence surfaces
- Changes:
  - Explicitly separate immutable soul payload vs adaptive personality payload.
  - Add schema-safe extension points for future crystallization paths.

## Risks and Rollback
- Risk: Over-engineering path system can slow immediate delivery.
  - Mitigation: Ship path registry with only initial paths, keep interface minimal.
- Risk: Ceremony polish may increase completion time beyond 7 minutes.
  - Mitigation: Include skip/compact mode for repeat users.
- Risk: Provider hard requirement may block air-gapped environments.
  - Mitigation: Add explicit policy toggle only if you approve offline exceptions later.
- Rollback:
  - Feature-flag crystallization path registry behind default path.
  - Keep legacy direct path callable until path registry stabilizes.

## Verification Checklist for Chunk 01
- Birth completes within 3-7 minute target under normal conditions.
- Provider gating blocks continuation until valid provider is configured.
- Key presentation keeps auto-save and explicit acknowledgment.
- Final emergence has cinematic milestone sequence with clear completion.
- Crystallization path output always yields immutable soul + adaptive personality objects.
- Repair screen preserves balanced visibility and explicit consequences.

## Deferred Questions (Resolved)
- DQ-01: Default profile at install: Home default (office selectable later).
- DQ-02: Image-based path source: bundled local archetype assets first.
- DQ-03: Personality adaptation cadence: daily.
- DQ-04: Office templates/admin locks: defer to Chunk 4+ after core birth flow stabilizes.
