# Genesis Quest Continuation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate old First Signal work safely and extend Genesis Quest with private diagnostics, fair trials, Runtime Presence, compacted activity, and seasonal/family progression.

**Architecture:** Hive remains the canonical owner of Presence and journey data. The frontend only decodes historical browser state and requests an explicit migration; Hive performs idempotent canonical writes. Runtime diagnostics and Presence use narrow typed HTTP projections, while compaction and long-arc quests operate solely on redacted durable facts.

**Tech Stack:** Rust 2021, Axum, embedded SurrealDB through `abigail-persistence`, React 19, TypeScript 6, Vitest, PowerShell.

## Global Constraints

- PR #305's `PresenceCard` and `PresenceChoice` schema is canonical.
- Soul Forge ethics weights are never exposed through UI, API, logs, diagnostics, catalog data, or public docs.
- Progress events never copy message, reply, prompt, model output, tool argument/result, or memory content.
- Migrations never allocate or retry an Entity and exact-name candidates always require mentor selection.
- Quests never send text or execute skills.
- Work remains on `cursor/genesis-quest-continuation-5d0b`, based on `cursor/genesis-quest-gamification-4871`.
- The pull request remains draft and no parent, `beta`, or `main` branch is pushed or merged.

---

### Task 1: Legacy First Signal migration

**Files:**
- Create: `hive-app/src-ui/src/lib/legacyFirstSignal.ts`
- Create: `hive-app/src-ui/src/lib/legacyFirstSignal.test.ts`
- Modify: `hive-app/src-ui/src/components/GenesisFirstSignal.tsx`
- Modify: `hive-app/src-ui/src/components/GenesisFirstSignal.test.tsx`
- Modify: `hive-app/src-ui/src/lib/questClient.ts`
- Modify: `crates/hive-daemon/src/journey.rs`
- Modify: `crates/hive-daemon/src/app.rs`
- Modify: `crates/hive-daemon/src/access.rs`
- Modify: `crates/abigail-persistence/src/schema.rs`
- Modify: `docs/ENTITY_GENESIS.md`

**Interfaces:**
- Produces: `readLegacyFirstSignalJournal(): LegacyFirstSignalJournal | null`
- Produces: `resolveLegacyCandidates(journal): Promise<LegacyCandidate[]>`
- Produces: `migrateLegacyFirstSignal(entityId, request): Promise<LegacyMigrationResult>`
- Produces: `POST /v1/entities/:id/journey/legacy-first-signal`

- [ ] **Step 1: Write strict frontend decoder tests**

Cover the exact historical v1 session-storage and v2 local-storage shapes,
malformed-byte retention, conflicting-record retention, keyed receipt lookup,
unkeyed baseline filtering, and no mutation during reads.

Run:

```bash
cd hive-app/src-ui
npx vitest run src/lib/legacyFirstSignal.test.ts
```

Expected: FAIL because `legacyFirstSignal.ts` does not exist.

- [ ] **Step 2: Implement the read-only decoder and candidate resolver**

Use the historical keys and closed profile:

```ts
interface LegacyProfile {
  name: string;
  purpose: string;
  temperament: "gentle" | "direct" | "curious";
  boundaries: string;
  identity: "aether" | "ember" | "lumen";
}
```

Map it to canonical Presence with `crest: "orbit"` and `motion: "calm"`.
Never POST creation. Keyed attempts may only resolve a `complete` receipt;
unkeyed attempts return exact-name, non-Hive, post-baseline candidates.

- [ ] **Step 3: Write failing Hive migration tests**

In `journey.rs`, test:

- first import creates canonical `PresenceCard` and journey badges;
- replay returns the same result without changing timestamps;
- response-loss replay completes a partial import;
- one migration key cannot bind another Entity or request digest;
- an old five-field `presence.json` can be replaced only when it matches the
  reviewed import;
- a later canonical Presence edit is not overwritten;
- serialized request/response contain no scoring or narrative-event keys.

Run:

```bash
cargo test -p hive-daemon journey::tests::legacy_first_signal
```

Expected: FAIL because the migration API and ledger do not exist.

- [ ] **Step 4: Implement the migration ledger and route**

Add a schema-full `genesis_legacy_migration` table containing only owner,
Entity, request digest, state, and timestamps. Compose Presence server-side,
award applicable badges monotonically, and mark complete only after both
canonical writes succeed. Hash the opaque old attempt into the record ID.

- [ ] **Step 5: Add the explicit recovery panel**

`GenesisFirstSignal` must show the retained old brief, explain candidate
ambiguity, offer Check/Review/Migrate actions, and block a new launch while
unresolved. Clear only the exact old journal after a successful canonical
response; write a resolution marker before removing v1 session storage.

- [ ] **Step 6: Run focused migration suites**

```bash
cargo test -p abigail-persistence journey
cargo test -p hive-daemon legacy_first_signal
cd hive-app/src-ui && npx vitest run src/lib/legacyFirstSignal.test.ts src/components/GenesisFirstSignal.test.tsx
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/abigail-persistence/src/schema.rs crates/hive-daemon/src hive-app/src-ui/src docs/ENTITY_GENESIS.md
git commit -m "feat(genesis): migrate legacy First Signal journeys"
```

### Task 2: Redacted quarantine diagnostics

**Files:**
- Modify: `crates/entity-core/src/lib.rs`
- Modify: `crates/entity-daemon/src/outbox.rs`
- Modify: `crates/entity-daemon/src/routes.rs`
- Modify: `crates/entity-daemon/src/main.rs`
- Modify: `docs/ENTITY_GENESIS.md`

**Interfaces:**
- Produces: `JourneyQuarantineDiagnostic`
- Produces: `RuntimeOutbox::quarantine_diagnostics()`
- Produces: `GET /v1/outbox/quarantine`

- [ ] **Step 1: Write failing serialization and route tests**

Assert the exact keys are `record_id`, `kind`, `quarantined_at_utc`, and
`reason`. Recursively reject `weights`, scoring axes, `message`, `reply`,
`content`, `prompt`, `payload`, `model_output`, `soul_ref`, and
`payload_digest`.

Run:

```bash
cargo test -p entity-daemon quarantine_diagnostic
```

Expected: FAIL because the projection is absent.

- [ ] **Step 2: Implement the narrow projection and endpoint**

Map persisted quarantine rows into the public DTO while retaining the full
reservation only inside `RuntimeOutbox`. Keep the reason fixed and sort by
quarantine timestamp and record ID for deterministic diagnostics.

- [ ] **Step 3: Run focused tests and commit**

```bash
cargo test -p entity-core -p entity-daemon quarantine
git add crates/entity-core/src/lib.rs crates/entity-daemon/src docs/ENTITY_GENESIS.md
git commit -m "feat(runtime): add redacted journey quarantine diagnostics"
```

### Task 3: Non-dominating Soul Forge trials

**Files:**
- Modify: `crates/soul-forge/src/lib.rs`
- Modify: `docs/ENTITY_GENESIS.md`

**Interfaces:**
- Produces test-only audit of built-in scenarios; no public scoring API.

- [ ] **Step 1: Add the failing balance audit**

For every scenario and ordered pair of choices, assert neither choice is at
least as high on every internal factor and strictly higher on one. Also assert
equal total influence within a small floating-point tolerance.

Run:

```bash
cargo test -p soul-forge built_in_scenarios_offer_tradeoffs_without_a_dominant_answer
```

Expected: FAIL on unequal influence totals.

- [ ] **Step 2: Rebalance internal values minimally**

Adjust only built-in internal calibration values. Do not modify any presentation
DTO, label, description, log, or response.

- [ ] **Step 3: Document the no-best-answer principle and commit**

```bash
cargo test -p soul-forge
git add crates/soul-forge/src/lib.rs docs/ENTITY_GENESIS.md
git commit -m "fix(ethics): keep every Soul Forge trial answer viable"
```

### Task 4: Animated Presence crest in Entity Runtime

**Files:**
- Modify: `crates/hive-daemon/src/access.rs`
- Modify: `crates/entity-daemon/src/hive_client.rs`
- Modify: `crates/entity-daemon/src/routes.rs`
- Modify: `crates/entity-daemon/src/main.rs`
- Create: `entity-runtime-app/src-ui/src/lib/presence.ts`
- Create: `entity-runtime-app/src-ui/src/components/PresenceCrest.tsx`
- Create: `entity-runtime-app/src-ui/src/components/PresenceCrest.test.tsx`
- Modify: `entity-runtime-app/src-ui/src/components/ChatPanel.tsx`
- Modify: `entity-runtime-app/src-ui/src/components/ChatPanel.test.tsx`
- Modify: `entity-runtime-app/src-ui/src/index.css`

**Interfaces:**
- Produces: `GET /v1/presence` on Entity Runtime
- Produces: `getRuntimePresence(baseUrl, signal)`
- Produces: `PresenceCrest`

- [ ] **Step 1: Write failing access and UI tests**

Assert a Runtime lease reads only its own Presence, desktop writes remain
unchanged, the header renders the canonical crest, missing Presence is harmless,
and `matchMedia("(prefers-reduced-motion: reduce)")` changes animated motion to
`still`.

- [ ] **Step 2: Add the Entity-scoped proxy**

Allow `GET /v1/entities/:id/presence` under `Access::Entity`, add a typed
lease-authenticated Hive client call, and expose it from Entity daemon without
accepting an Entity ID from the browser.

- [ ] **Step 3: Add the compact header crest**

Render the canonical SVG beside the Entity name. Keep every visual state
text-redundant and listen for live reduced-motion preference changes.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p hive-daemon access
cargo test -p entity-daemon presence
cd entity-runtime-app/src-ui && npm test && npm run build
git add crates/hive-daemon/src/access.rs crates/entity-daemon/src entity-runtime-app/src-ui/src
git commit -m "feat(runtime): carry canonical Presence into Entity chat"
```

### Task 5: Journey event compaction

**Files:**
- Modify: `crates/abigail-persistence/src/schema.rs`
- Modify: `crates/hive-daemon/src/journey.rs`
- Modify: `docs/ENTITY_GENESIS.md`

**Interfaces:**
- Produces: `JourneyEventCompactionRecord`
- Produces: `JourneyStore::compact_runtime_events`

- [ ] **Step 1: Write failing compaction safety tests**

Cover projection equivalence, snapshot-before-delete interruption, replay,
collision detection, owner isolation, season-boundary retention, and recursive
content/scoring leak rejection.

- [ ] **Step 2: Implement compact snapshots**

Store counters plus exact record-ID/digest pairs in one schema-full,
owner-scoped record per Entity. Write/merge the snapshot before deleting covered
rows. Load the snapshot during event prevalidation and projection.

- [ ] **Step 3: Add conservative automatic triggering**

After successful projection, compact only when the row threshold is exceeded
and only rows older than the current UTC quarter start. A compaction failure
must not fail outbox acknowledgement after the projection is durable; report it
as a redacted warning and retry on a later reconciliation.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p abigail-persistence journey
cargo test -p hive-daemon compaction
git add crates/abigail-persistence/src/schema.rs crates/hive-daemon/src/journey.rs docs/ENTITY_GENESIS.md
git commit -m "feat(journey): compact old redacted activity events"
```

### Task 6: Seasonal and family quests

**Files:**
- Modify: `crates/hive-core/src/genesis_quest.rs`
- Modify: `crates/hive-daemon/src/genesis.rs`
- Modify: `crates/hive-daemon/src/journey.rs`
- Modify: `hive-app/src-ui/src/lib/questClient.ts`
- Modify: `hive-app/src-ui/src/test/questFixture.ts`
- Modify: `hive-app/src-ui/src/components/GenesisQuest.tsx`
- Modify: `hive-app/src-ui/src/components/GenesisQuest.test.tsx`
- Modify: `hive-app/src-ui/src/components/Genesis.css`
- Modify: `docs/ENTITY_GENESIS.md`

**Interfaces:**
- Produces: `LONG_ARC_QUEST_DEFINITIONS`
- Produces: `LongArcQuestView`
- Adds: `long_arc_quests` to catalog/journey views.

- [ ] **Step 1: Write failing core and daemon tests**

Assert stable UTC-quarter boundaries, Entity seasonal progress, owner-scoped
family aggregation across at least two Entities, reset at a new season, and
content-free/weight-free serialization.

- [ ] **Step 2: Implement current-season activity summaries**

Filter durable mentor-turn metadata timestamps to the current quarter. Seasonal
quests must not read conversation content or redacted journey event timestamps.
Aggregate family counts only over registered non-Hive Entities owned by the
current household.

- [ ] **Step 3: Render long-arc quest cards**

Show title, scope, season label, progress, and completion after awakening.
Include the existing no-auto-send copy and no action button.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p hive-core genesis_quest
cargo test -p hive-daemon long_arc
cd hive-app/src-ui && npx vitest run src/components/GenesisQuest.test.tsx
git add crates/hive-core/src/genesis_quest.rs crates/hive-daemon/src hive-app/src-ui/src docs/ENTITY_GENESIS.md
git commit -m "feat(journey): add seasonal and family quests"
```

### Task 7: Windows split-stack compatibility

**Files:**
- Create: `scripts/tests/test_launch_split_stack_contract.mjs`
- Create: `scripts/tests/test_launch_split_stack_environment.ps1`
- Create: `scripts/dev/start_process_with_environment_overlay.ps1`
- Modify: `scripts/dev/launch_split_stack.ps1`
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`

- [ ] **Step 1: Write failing wiring and Windows PowerShell 5.1 contracts**

Keep the Node contract focused on helper wiring, required Hive/Runtime child
variables, documentation, and CI execution. The Windows PowerShell contract
parses launcher/helper syntax with the PowerShell AST, rejects
`Start-Process`/`start`/`saps` environment parameters and splats, starts a
synthetic child, and verifies its overlay without changing parent values.

- [ ] **Step 2: Implement the PowerShell 5.1-safe helper**

Build a fresh `System.Diagnostics.ProcessStartInfo`, set
`UseShellExecute = $false`, and apply only required nonempty values through its
child environment before calling `System.Diagnostics.Process.Start`. The helper
does not change the parent process environment, so present nonempty, present
empty, and absent parent variables remain exact by construction on success and
failure. Do not use P/Invoke, `Add-Type`, parent writes, restore logic, or value
logging.

- [ ] **Step 3: Add the contract to CI and commit**

```bash
node --test scripts/tests/test_launch_split_stack_contract.mjs
git add scripts/dev/launch_split_stack.ps1 scripts/dev/start_process_with_environment_overlay.ps1 scripts/tests/test_launch_split_stack_contract.mjs scripts/tests/test_launch_split_stack_environment.ps1 .github/workflows/ci.yml README.md docs/superpowers
git commit -m "fix(windows): launch split desktop apps on PowerShell 5"
```

### Task 8: Full verification and handoff

**Files:**
- Modify if needed: `docs/ENTITY_GENESIS.md`
- Modify if needed: draft pull-request description.

- [ ] **Step 1: Format and lint**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude abigail-app -- -D warnings
```

- [ ] **Step 2: Run the full Rust suite**

```bash
cargo test --workspace --exclude abigail-app
```

- [ ] **Step 3: Run every UI suite and production build**

```bash
cd hive-app/src-ui && npm test && npm run build
cd ../../entity-runtime-app/src-ui && npm test && npm run build
cd ../../tauri-app/src-ui && npm test && npm run build && npm run check:command-contract
```

- [ ] **Step 4: Run Windows and leak contracts**

```bash
node --test scripts/tests/test_launch_split_stack_contract.mjs
cargo test -p hive-daemon user_facing_production_routes_never_expose_hidden_weights
cargo test -p hive-daemon router_user_surfaces_hide_weights_and_full_birth_requires_matching_runtime
```

- [ ] **Step 5: Review, commit any verification-only docs, push, and update the draft PR**

```bash
git status --short
git diff --check
git push -u origin cursor/genesis-quest-continuation-5d0b
```

