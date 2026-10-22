# Genesis Quest Owner Decisions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:test-driven-development for every behavior change. Execute tasks in order because the frontend and progression tasks consume backend interfaces established earlier.

**Goal:** Implement canonical Presence plus resumable keyed creation, permanently hidden and versioned ethics scoring, a durable long-term mentoring arc, and the model-setup gate on PR #305.

**Architecture:** Keep Genesis Quest, Hive control-token auth, and `PresenceCard` as the stable core. Add a durable creation receipt ledger and keyed Genesis seed, split internal certificates from public summaries, and combine durable conversation summaries with redacted idempotent Runtime journey events. A slim First Signal welcome seeds the existing Quest rather than introducing a second creation application.

**Tech Stack:** Rust 2021, Axum, serde, SurrealKV, React 19, TypeScript, Vitest, Testing Library.

## Global Constraints

- Branch: `cursor/genesis-quest-gamification-4871` only.
- Full requirements: `docs/superpowers/specs/2026-10-22-genesis-quest-owner-decisions-design.md`.
- Never persist message, reply, prompt, model-output, or memory content in journey events.
- Never expose ethic weights or axis values through user-facing routes or frontend types.
- New certificate scoring version: `triangle-ethic-v1`.
- New persisted fields are backward-compatible with `serde(default)`.
- No automatic quest messages, skill use, or memory creation.

---

### Task 1: Durable creation receipts and keyed Genesis sessions

**Files:**
- Modify: `crates/abigail-core/src/global_config.rs`
- Create: `crates/abigail-identity/src/creation.rs`
- Modify: `crates/abigail-identity/src/lib.rs`
- Modify: `crates/abigail-identity/Cargo.toml`
- Modify: `crates/hive-core/src/lib.rs`
- Modify: `crates/hive-core/src/genesis.rs`
- Create: `crates/hive-daemon/src/genesis_creation.rs`
- Create: `crates/hive-daemon/src/creation_http_tests.rs`
- Modify: `crates/hive-daemon/src/{main.rs,app.rs,access.rs,routes.rs,genesis.rs,genesis_store.rs}`

**Interfaces:**
- `AgentCreationRecord { name, entity_id, state }`
- `AgentCreationState::{Reserved, Complete}`
- `AgentCreationStatus::{Absent, Reserved, Complete, Unavailable}`
- `IdentityManager::{entity_creation_status, create_agent_idempotent}`
- `GenesisCreationSeed { mode, name, purpose, presence }`
- `GenesisSession.creation_seed: Option<GenesisCreationSeed>`
- `GenesisStore::create_or_resume`
- `genesis_creation::ensure_entity_link`
- `GET /v1/entity-creations/:creation_key`

- [ ] **Step 1: Write failing receipt tests**

Add identity tests that concurrently call:

```rust
manager.create_agent_idempotent("Signal", &creation_key)
```

and assert one UUID, one registry entry, one persisted receipt, replay after a
new `IdentityManager`, name conflict, terminal unavailable after deletion, and
legacy `GlobalConfig` without `creation_attempts`.

- [ ] **Step 2: Verify receipt tests fail**

Run:

```bash
cargo test -p abigail-identity creation
```

Expected: compile failures for the missing types/methods.

- [ ] **Step 3: Implement the receipt ledger**

Add defaulted `BTreeMap<String, AgentCreationRecord>` to `GlobalConfig`. Extract
the existing agent-file initialization from `create_agent` so keyed and unkeyed
creation share it. Persist reservation before files and persist registration +
`Complete` in one cloned `GlobalConfig` write. Never remove receipt records when
an Entity is deleted.

- [ ] **Step 4: Add failing HTTP contract tests**

Tests must cover:

```text
POST /v1/entities {name, creation_key}
GET  /v1/entity-creations/:creation_key
```

including concurrent replay, bad UUID = 400, name conflict = 409, unavailable =
410, storage failure = 500, control-token access, and legacy no-key response
shape.

- [ ] **Step 5: Implement HTTP creation contracts**

Extend `CreateEntityRequest`/`CreateEntityResponse`; add discriminated
`EntityCreationResponse`. Register the lookup route as desktop/Hive-admin only
under the current access middleware.

- [ ] **Step 6: Add failing keyed Genesis tests**

Add tests proving:

```rust
create_or_resume(owner, key, same_seed) // returns same session
create_or_resume(owner, key, changed_seed) // error
ensure_entity_link(...) // same receipt UUID after interrupted session save
```

and that old sessions without `creation_seed` still deserialize.

- [ ] **Step 7: Implement keyed Genesis create/resume and activation**

Validate canonical UUID keys and immutable seed equality. Seed name, purpose,
and `QuestState.presence` at revision zero. Replace `activate`'s unkeyed
allocation/rollback block with `ensure_entity_link`, preserving all Quest forge,
Presence, awakening, deletion, and guard logic.

- [ ] **Step 8: Verify backend creation slice**

Run:

```bash
cargo fmt --all -- --check
cargo test -p abigail-core global_config
cargo test -p abigail-identity creation
cargo test -p hive-core genesis
cargo test -p hive-daemon creation
cargo test -p hive-daemon genesis
```

- [ ] **Step 9: Commit**

```bash
git add crates/abigail-core crates/abigail-identity crates/hive-core crates/hive-daemon
git commit -m "feat(genesis): add durable keyed creation receipts"
git push -u origin cursor/genesis-quest-gamification-4871
```

---

### Task 2: Slim First Signal and canonical Presence

**Files:**
- Create: `hive-app/src-ui/src/components/GenesisFirstSignal.tsx`
- Create: `hive-app/src-ui/src/components/GenesisFirstSignal.test.tsx`
- Create: `hive-app/src-ui/src/lib/genesisLaunchDraft.ts`
- Create: `hive-app/src-ui/src/lib/genesisLaunchDraft.test.ts`
- Modify: `hive-app/src-ui/src/components/{GenesisWorkspace.tsx,GenesisWorkspace.test.tsx,Genesis.css}`
- Modify: `hive-app/src-ui/src/lib/{genesisClient.ts,genesisClient.test.ts}`
- Modify: `crates/hive-daemon/src/journey.rs`

**Interfaces:**
- `GenesisLaunchDraft { version: 1, creation_key, seed }`
- `createOrResumeGenesis(seed, creationKey)`
- `GenesisFirstSignal.onBegin(seed)`

- [ ] **Step 1: Write failing Presence write-cap test**

Assert `save_presence` refuses encoded bytes over `MAX_PRESENCE_BYTES` before
calling atomic write and leaves an existing card unchanged.

- [ ] **Step 2: Implement symmetric Presence cap**

Keep `PresenceCard`/`PresenceChoice`; do not add `presence_profile.rs`. Check the
serialized byte length in `save_presence`.

- [ ] **Step 3: Write failing launch-journal tests**

Test valid round trip, malformed/unknown-version retention, immutable key,
seed mismatch, clear-after-exact-match, and serialized concurrent access. The
journal key is `abigail.first-signal.genesis.v1`.

- [ ] **Step 4: Implement launch journal and keyed client**

Write the journal before POST. Use `navigator.locks` when available and a module
promise lock otherwise. Keep the journal after network/error outcomes; clear it
only after the returned session ID equals the creation key.

- [ ] **Step 5: Write failing First Signal component tests**

Assert the form:

- requires trimmed name and purpose;
- previews the current crest;
- submits only the canonical seed fields;
- resumes a saved journal;
- never sends permission/runtime fields;
- enters the existing Quest after success.

- [ ] **Step 6: Implement slim First Signal**

Replace the mode-card launch hero with name, purpose, temperament, palette,
crest, and motion controls using current Quest constants and `PresenceCrest`.
Keep the saved-session list and all existing workspace tabs/actions.

- [ ] **Step 7: Verify frontend creation slice**

Run:

```bash
cd hive-app/src-ui
npx vitest run src/components/GenesisFirstSignal.test.tsx src/components/GenesisWorkspace.test.tsx src/lib/genesisLaunchDraft.test.ts src/lib/genesisClient.test.ts
npm run build
```

- [ ] **Step 8: Commit**

```bash
git add crates/hive-daemon/src/journey.rs hive-app/src-ui/src
git commit -m "feat(hive-app): add keyed First Signal welcome"
git push -u origin cursor/genesis-quest-gamification-4871
```

---

### Task 3: Hidden, versioned ethics

**Files:**
- Modify: `crates/hive-core/src/lib.rs`
- Modify: `crates/hive-daemon/src/{birth.rs,journey.rs,access.rs,routes.rs}`
- Modify: `crates/hive-daemon/src/genesis.rs` tests
- Modify: `hive-app/src/lib.rs`
- Modify: `hive-app/src-ui/src/{lib/questClient.ts,components/GenesisCelebration.tsx}`

**Interfaces:**
- `SCORING_VERSION = "triangle-ethic-v1"`
- `BirthCertificate.scoring_version`
- `BirthCertificate.payload_version = 3` for new issuance
- `BirthCertificateSummary` without weights
- injective version-3 issuance plus raw version-1/version-2 compatibility

- [ ] **Step 1: Write failing serialization leak test**

Create a recursive JSON walker that rejects these keys:

```text
weights
deontology
teleology
areteology
welfare
```

Exercise Genesis create/load/action, quest catalog, journey, Presence GET/PUT,
birth scenarios, and generic birth POST. The test must fail first on journey and
birth POST.

- [ ] **Step 2: Write failing certificate-version tests**

Assert new certificates contain `payload_version = 3` and
`scoring_version = "triangle-ethic-v1"`. Delimiter/newline injection and any
tampered displayed field, scoring version, or hidden weight must invalidate the
v3 signature. Prove original raw v1 and prior scoring-stamped raw v2 fixtures
still verify.

- [ ] **Step 3: Implement internal/public certificate split**

Keep `BirthCertificate` full for disk/runtime. Add `BirthCertificateSummary`.
Sign new certificates with the injective, length-prefixed v3 payload containing
all displayed fields, scoring version, and all four hidden weights. Select raw
v1/v2 compatibility based on persisted key presence, and reject unsupported
explicit payload versions.

- [ ] **Step 4: Gate non-Genesis routes**

Return summaries from generic birth POST and journey. Make GET birth
Entity-Runtime-only in `access.rs`. Remove unused Hive Tauri
`get_birth_document`; retain the Entity daemon's lease-scoped full document.

- [ ] **Step 5: Remove frontend weight state**

Delete `weights` from `CertificateView`; celebration shows archetype, epithet,
birth path, scoring version, and signature status only.

- [ ] **Step 6: Verify hidden ethics slice**

Run:

```bash
cargo test -p hive-core certificate
cargo test -p hive-daemon weight
cargo test -p hive-daemon certificate
cargo test -p hive-daemon family_entity_lease_reaches_only_its_own_runtime_routes
cd hive-app/src-ui && npm test && npm run build
```

- [ ] **Step 7: Commit**

```bash
git add crates/hive-core crates/hive-daemon crates/entity-daemon hive-app
git commit -m "fix(ethics): hide and version scoring weights"
git push -u origin cursor/genesis-quest-gamification-4871
```

---

### Task 4: Data-driven long-term journey

**Files:**
- Modify: `crates/hive-core/src/genesis_quest.rs`
- Modify: `crates/abigail-persistence/src/schema.rs`
- Modify: `crates/hive-daemon/src/{journey.rs,routes.rs,runtime_registry.rs}`
- Modify: `crates/entity-daemon/src/{routes.rs,pipeline/journal.rs}`
- Modify: `hive-app/src-ui/src/{lib/questClient.ts,test/questFixture.ts,components/GenesisQuest.tsx,components/EntityJourneyChip.tsx}`

**Interfaces:**

```rust
JourneyActivity {
    mentor_messages,
    conversation_sessions,
    active_days,
    active_weeks,
    successful_skill_uses,
    confirmed_memories,
}
```

Catalog tiers:

```text
Spark 0
Kindling 80
Ember 180
Hearthfire 350
Beacon 600
Constellation 950
Legacy 1400
```

Creation XP totals at most 120. New activity badges:

```text
FirstWords messages 1 / 15 XP
GettingAcquainted messages 5 / 25
Kindred messages 25 / 40
HearthRegular messages 100 / 80
TrustedCompanion messages 500 / 180
ReturningCompanion sessions 5 / 50
SeasonedCompanion sessions 25 / 120
SteadyRhythm active_days 7 / 80
HearthSeason active_weeks 4 / 120
ConstellationKeeper active_weeks 12 / 220
SkillExplorer skill_uses 5 / 80
SkillWeaver skill_uses 25 / 160
MemoryKeeper confirmed_memories 3 / 80
LivingArchive confirmed_memories 15 / 180
```

- [ ] **Step 1: Write failing catalog/curve tests**

Assert every badge has one definition, tier floors increase, all creation badges
sum to 120, creation reaches Kindling but not Ember, and catalog serializes
tiers plus activity requirements.

- [ ] **Step 2: Implement data-driven definitions**

Replace title/hint/XP/target matches and `LEVELS` with `BadgeDefinition` and
`BondTierDefinition` tables. Add enum variants and generic metric evaluation.

- [ ] **Step 3: Write failing durable-event tests**

Assert:

- duplicate Runtime record IDs count once;
- owner/Entity mismatch fails;
- unknown kinds do not affect progression;
- stored rows contain no `message`, `reply`, `content`, `prompt`, or arbitrary
  `payload`;
- rows survive a new `JourneyStore`;
- concurrent skill/memory events do not lose counts.

- [ ] **Step 4: Implement `entity_journey_event`**

Add table and owner/entity indexes. Persist only:

```text
record_id, owner_id, entity_id, kind, occurred_at,
session_id?, skill_id?, tool_name?, successful_skill_count?,
memory_id?, memory_weight?
```

Only `skill_used` and `memory_confirmed` are tracked. Use the outbox record ID as
the table record ID and serialize writes under JourneyStore's lock. New skill
events use `session_id` plus a bounded positive aggregate count; retain
`skill_id` and `tool_name` only to verify historical per-tool rows.

- [ ] **Step 5: Emit real Runtime events**

Reserve one durable outbox slot before entering a tool-capable turn, persist the
assistant fact before committing one aggregate `skill_used` record, and release
the reservation when no successful action commits. Reserve before an explicit
non-ephemeral memory insert, then commit `memory_confirmed`. Post-side-effect
commit failures keep the successful response and recover from durable,
content-free assistant or memory metadata. Never include message/reply/memory
content.

- [ ] **Step 6: Reconcile durable conversations**

Replace the 500-turn window with full conversation summaries after birth:
distinct user-turn count, session IDs, UTC dates, and ISO weeks. Merge these
facts with event counts into defaulted monotonic `EntityJourney` counters.

- [ ] **Step 7: Integrate outbox sync**

Validate the authenticated lease/entity first, sync the outbox, then persist
tracked journey summaries before returning success. Retry is safe because event
record IDs are idempotent.

- [ ] **Step 8: Update frontend catalog/activity**

Consume tier and milestone data from catalog, show extended counters and next
milestone, retain first-quest prompts as copy-only, and add no send buttons.

- [ ] **Step 9: Verify progression slice**

Run:

```bash
cargo test -p hive-core genesis_quest
cargo test -p hive-daemon journey
cargo test -p hive-daemon runtime
cargo test -p entity-daemon outbox
cargo test -p entity-daemon memory
cd hive-app/src-ui && npm test && npm run build
```

- [ ] **Step 10: Commit**

```bash
git add crates/hive-core crates/abigail-persistence crates/hive-daemon crates/entity-daemon crates/entity-core hive-app/src-ui/src
git commit -m "feat(genesis): extend journey through real runtime activity"
git push -u origin cursor/genesis-quest-gamification-4871
```

---

### Task 5: Model setup gate

**Files:**
- Modify: `hive-app/src-ui/src/App.tsx`
- Modify: `hive-app/src-ui/src/App.test.tsx`

- [ ] **Step 1: Write failing card tests**

Under `ready_state: "needs_provider"` and `awaiting[entity.id]` assert:

```text
subtitle = Connect a model to continue Genesis
button   = Connect a model to continue
click    = Provider Wizard opens
GenesisWorkspace does not open
```

After mocked provider completion/status refresh, assert ordinary Continue
Genesis routing returns.

- [ ] **Step 2: Implement the gate**

Branch card copy/click behavior on `needsProvider && awaiting[id]`. Keep the new
First Signal create card available. Do not call `handleOpen` for the gated card.

- [ ] **Step 3: Verify and commit**

```bash
cd hive-app/src-ui
npx vitest run src/App.test.tsx
npm run build
cd ../../..
git add hive-app/src-ui/src/App.tsx hive-app/src-ui/src/App.test.tsx
git commit -m "fix(hive-app): route model-less Genesis cards to setup"
git push -u origin cursor/genesis-quest-gamification-4871
```

---

### Task 6: Documentation, complete verification, and PR update

**Files:**
- Modify: `docs/ENTITY_GENESIS.md`
- Modify: `AGENTS.md`
- Modify: `CLAUDE.md` only where permanent constraints changed
- Modify: PR #305 body through the PR tool

- [ ] **Step 1: Update product/architecture documentation**

Record all four owner decisions, scoring version, keyed receipts, event privacy,
exact progression curve, model gate, acceptance limits, and deferred old First
Signal recovery pieces.

- [ ] **Step 2: Run focused secret/leak scans**

```bash
rg -n '"weights"|deontology|teleology|areteology|welfare' hive-app/src-ui/src crates/hive-daemon/src
rg -n 'message|reply|content|prompt|payload' crates/hive-daemon/src/journey.rs
```

Review every result against the internal/runtime allowlist.

- [ ] **Step 3: Run full verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude abigail-app -- -D warnings
cargo test --workspace --exclude abigail-app
cd hive-app/src-ui && npm run build && npm test
cd ../../entity-runtime-app/src-ui && npm run build && npm test
```

- [ ] **Step 4: Final branch review**

Confirm a clean worktree, remote HEAD equality, PR-only diff, no secrets, no
changes to `beta`/`main`, and no merge.

- [ ] **Step 5: Update PR body and comment**

Replace decision questions with implemented decisions and remaining slices.
Update test counts from the fresh runs. Do not post another review comment
unless the user requested one; the prior independent-review comment remains the
single review comment.

- [ ] **Step 6: Commit and push docs**

```bash
git add AGENTS.md CLAUDE.md docs/ENTITY_GENESIS.md
git commit -m "docs: record Genesis owner decisions and validation"
git push -u origin cursor/genesis-quest-gamification-4871
```
