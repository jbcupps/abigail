# Final Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close every final-review finding without weakening Genesis ownership, privacy, receipt idempotency, certificate compatibility, or Hive-only persistence.

**Architecture:** Keep retries idempotent at the existing Hive HTTP boundary, derive receipt recovery links only in serialized Genesis views, and introduce explicit certificate payload version 3 while retaining raw v1/v2 dispatch. Make archive publication transactional under the registry lock, and put the one Entity-name rule in `abigail-core` so all creation and rename paths share it.

**Tech Stack:** Rust/Axum/SurrealDB, React/TypeScript/Vitest, Cargo test/Clippy/fmt.

## Global Constraints

- Work only on `cursor/genesis-quest-gamification-4871`; do not modify PR metadata, `beta`, or `main`.
- Use RED/GREEN for every behavior change and preserve all approved Genesis/Hive design boundaries.
- Only the Hive daemon opens durable embedded storage; Runtime retries use the same IDs.
- Receipt recovery and effective Entity links are presentation-only and never rewrite canonical Genesis data.
- New certificate signatures are injective and versioned; historical raw v1 and scoring-stamped v2 remain verifiable.
- Entity names are trimmed, contain 1 through 80 Unicode scalar values, and contain no control characters.
- Run focused tests for every cycle, then strict affected Clippy/fmt and both affected frontend suites.

---

### Task 1: Projection failure remains retryable

**Files:**
- Modify: `crates/hive-daemon/src/routes.rs`
- Modify: `crates/hive-daemon/src/access.rs`
- Modify: `crates/entity-daemon/src/main.rs`
- Modify: `crates/entity-daemon/src/outbox.rs`

**Interfaces:**
- Consumes: `ApiEnvelope<OutboxSyncResponse>` and existing record-ID deduplication.
- Produces: an error envelope on projection failure; Runtime keeps its exact records until a successful replay acknowledges them.

- [ ] Add a real HTTP regression that injects one projection write failure, expects `ok: false`, retries the identical JSON, then asserts one projected event and one execution receipt.
- [ ] Extract the production Runtime sync-result handling into a small function and test that an error keeps the same queued ID/depth while success removes it.
- [ ] Run the focused Hive and Entity tests and capture the expected RED results.
- [ ] Return `ApiEnvelope::error` when projection reconciliation fails, leaving the already durable event/control-plane receipt available for exact replay.
- [ ] Run the same focused tests for GREEN and commit/push.

### Task 2: Receipt-only Genesis card link

**Files:**
- Modify: `crates/hive-daemon/src/genesis.rs`
- Modify: `hive-app/src-ui/src/lib/genesisClient.ts`
- Modify: `hive-app/src-ui/src/components/GenesisEntityPortrait.tsx`
- Modify: `hive-app/src-ui/src/components/GenesisEntityPortrait.test.ts`
- Modify: `hive-app/src-ui/src/App.test.tsx`

**Interfaces:**
- Produces: `GenesisSession.effective_entity_id?: string | null`, serialized from canonical `definition.entity_id` or the durable creation receipt.
- Consumes: the effective ID in portrait and awaiting-session maps.

- [ ] Add a backend test that interrupts canonical link persistence and asserts list/load views expose `effective_entity_id` while `definition.entity_id` remains null.
- [ ] Add frontend tests whose receipt-only session maps to the Entity card, opens Provider Wizard in `needs_provider`, resumes Genesis in `ready`, and never invokes generic setup.
- [ ] Run focused backend/Vitest tests for RED.
- [ ] Derive `effective_entity_id` in `view`, type it, and use it for card maps without mutating the stored session.
- [ ] Run focused tests for GREEN and commit/push.

### Task 3: Injective certificate payload v3

**Files:**
- Modify: `crates/hive-core/src/lib.rs`
- Modify: `crates/hive-daemon/src/birth.rs`
- Modify: `docs/ENTITY_GENESIS.md`
- Modify: `AGENTS.md`

**Interfaces:**
- Produces: defaulted `payload_version: u8`; new certificates set `3`.
- Preserves: absent version plus absent `scoring_version` dispatches raw v1; absent version plus present `scoring_version` dispatches prior v2.

- [ ] Add compatibility fixtures for v1, prior v2, and v3; add delimiter/newline collision and all-field tamper tests.
- [ ] Run focused birth tests for RED.
- [ ] Encode v3 as a fixed version prefix followed by length-prefixed UTF-8 fields and fixed-width weight bits; reject explicit unsupported versions.
- [ ] Issue v3 for all new certificates and update current-version docs.
- [ ] Run focused tests for GREEN and commit/push.

### Task 4: Transactional archive ordering

**Files:**
- Modify: `crates/abigail-identity/src/lib.rs`

**Interfaces:**
- Consumes: one held `global_config` write guard.
- Produces: preflighted move followed by cloned registry persistence/publication; save failure renames the archive back.

- [ ] Add tests for missing source, destination collision, injected rename failure, and injected config-save failure with successful rollback; assert registry and creation receipt state on each failure.
- [ ] Run focused identity tests for RED.
- [ ] Add a private injectable archive helper used by production `archive_agent`, preflight source/destination, move while locked, save the cloned next registry, roll back on save error, then publish memory.
- [ ] Run focused tests for GREEN and commit/push.

### Task 5: Canonical Entity-name validator

**Files:**
- Add: `crates/abigail-core/src/entity_name.rs`
- Modify: `crates/abigail-core/src/lib.rs`
- Modify: `crates/abigail-identity/src/creation.rs`
- Modify: `crates/abigail-identity/src/lib.rs`
- Modify: `crates/hive-core/Cargo.toml`
- Modify: `crates/hive-core/src/genesis.rs`
- Modify: `crates/hive-daemon/src/genesis.rs`

**Interfaces:**
- Produces: `abigail_core::validate_entity_name(&str) -> Result<(), String>`.
- Applies to: keyed/unkeyed identity creation, direct Genesis name edits, Genesis creation seeds, activation, and registry renaming.

- [ ] Add table tests for whitespace, newline/control, 81 ASCII characters, and exactly 80 multibyte characters at the core and integration entry points.
- [ ] Add a compatibility test proving a previously stored invalid draft can be viewed but activation fails before config/registry mutation.
- [ ] Run focused core/identity/Genesis tests for RED.
- [ ] Replace duplicate validators and validate before every listed mutation; keep stored-draft decoding compatible while blocking new invalid name updates and activation.
- [ ] Run focused tests for GREEN and commit/push.

### Task 6: FirstLight, schema migration, and recovery wording

**Files:**
- Modify: `crates/hive-daemon/src/journey.rs`
- Modify: `crates/hive-daemon/src/genesis.rs`
- Modify: `crates/abigail-persistence/src/schema.rs`
- Modify: `docs/ENTITY_GENESIS.md`
- Modify: `AGENTS.md`
- Modify: `CLAUDE.md` if its invariant wording requires alignment.

**Interfaces:**
- FirstLight source: confirmed Genesis awakening/session link only.
- Migration contract: every allowed field is byte-for-byte/value-for-value preserved while every forbidden field is absent.

- [ ] Add a generic-born journey test that does not award FirstLight and retain the linked-awakening positive test.
- [ ] Strengthen the migration test to compare all ten allowed fields to the pre-migration row and assert every injected forbidden field is absent.
- [ ] Run focused tests for RED.
- [ ] Remove generic `birth_complete` from FirstLight derivation, pass only confirmed Genesis linkage, and correct allocation-recovery/badge docs.
- [ ] Run focused tests for GREEN and commit/push.

### Task 7: Aggregate verification and report

**Files:**
- Create: `.superpowers/sdd/final-review-fix-report.md`

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run strict Clippy for every affected crate with `--all-targets -- -D warnings`.
- [ ] Run affected Rust suites, Hive frontend tests/build, and Entity Runtime frontend tests/build.
- [ ] Re-read every finding against the final diff; inspect retry IDs/counts/receipts, canonical-vs-effective links, raw certificate dispatch, archive rollback, and every name entry point.
- [ ] Write exact RED/GREEN commands/results, commits, concerns, and acceptance limits to the requested report.
- [ ] Commit any final tracked corrections/report artifact and push only the current branch.
