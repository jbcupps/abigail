# Genesis Quest Owner Decisions Design

## Goal

Complete the four owner decisions for PR #305 without importing the obsolete
standalone First Signal application, weakening Hive authentication, exposing
hidden ethics scoring, or turning mentoring quests into automatic messages.

## Global constraints

- Work only on `cursor/genesis-quest-gamification-4871`.
- `PresenceCard` and `PresenceChoice` in `hive-core::genesis_quest` are the
  canonical Presence schema.
- Presence is presentation-only, remains outside `docs/`, never enters prompts,
  and never grants authority.
- A creation key is a canonical UUID and permanently identifies one creation
  attempt and at most one Entity UUID.
- Ethics weights, axis values, and scoring structures are absent from every
  user-facing Genesis, quest, journey, Presence, catalog, and generic birth
  response.
- The full hidden certificate remains available only to the Entity Runtime
  lease and to local signed/runtime files.
- Every signed birth certificate records an immutable scoring version. Existing
  certificates deserialize to version 1.
- Journey events contain no message content, reply content, memory content,
  prompts, model output, or arbitrary Runtime payload.
- Progress comes only from durable Runtime facts; quests never send messages or
  execute skills automatically.
- New journey and creation fields use `serde(default)` or equivalent defaults
  so existing profiles and records continue loading.

## 1. Canonical Presence and durable creation

### Presence

`PresenceCard` remains the only persisted `presence.json` type. The smaller
`presence_profile.rs` schema from the unmerged branch is not copied. Its useful
storage properties are folded into `journey.rs`: both reads and writes enforce
the 32 KiB byte limit, only missing files produce an empty result, malformed or
oversized files fail without replacement, and writes remain atomic.

The slim First Signal welcome uses the existing Presence enums, current
`PresenceCrest`, and unlocked default choices. It collects:

- canonical name;
- canonical purpose;
- temperament;
- palette;
- crest;
- motion.

Name and purpose seed the Genesis definition. Presence choices seed
`QuestState.presence` through `PresenceCard::compose`, so display text still
comes from the definition rather than client-owned strings.

### Keyed Genesis sessions

The Hive frontend creates a canonical UUID before its first creation request and
writes a small local journal before dispatch:

```text
{ version: 1, creation_key, mode, name, purpose, presence }
```

The server accepts `creation_key` and a typed `GenesisCreationSeed`. The key is
also the Genesis session ID. Creating the same key with the same seed returns
the existing session. Reusing a key with a different seed is a conflict. A
successful response clears the local dispatch journal because the durable
Genesis session is then discoverable through the normal session list.

Browser serialization uses a Web Lock when available and a process-local
fallback lock otherwise. The old branch's legacy candidate matching,
presentation-only direct creation, and multi-version browser draft migration
are deferred.

### Durable Entity receipts

`GlobalConfig` gains a defaulted `creation_attempts` map:

```text
creation_key -> { name, entity_id, reserved|complete }
```

Reservation and completion are atomically persisted with the registry.
Concurrent or restarted replays return the same Entity. A completed receipt
whose Entity was deleted becomes terminal `unavailable`; replay never creates a
replacement.

Genesis activation allocates with `create_agent_idempotent(name, session.id)`.
If linking the Entity ID into Genesis persistence is interrupted, a retry
recovers the receipt's original Entity ID. Generic `POST /v1/entities` may
optionally use the same key and `GET /v1/entity-creations/:key` reports
`absent`, `reserved`, `complete`, or `unavailable`. These routes remain under
the existing Hive control-token policy.

## 2. Hidden ethics and versioned certificates

The internal `BirthCertificate` keeps hidden weights for signed/runtime
artifacts. It gains:

```text
scoring_version: "triangle-ethic-v1"
```

New certificates carry explicit `payload_version: 3`. Their injective canonical
payload length-prefixes every field and binds all displayed fields,
`scoring_version`, and the exact hidden-weight bits. Verification continues to
accept raw version-1 certificates that lack both version keys and prior raw
version-2 certificates selected by `scoring_version` without an explicit
`payload_version`.

Public responses use `BirthCertificateSummary`, containing only:

- archetype;
- epithet;
- sigil;
- birth path;
- issued timestamp;
- scoring version;
- signature verification result where applicable.

`POST /v1/entities/:id/birth` and journey/celebration use the summary. The full
`GET /v1/entities/:id/birth` document becomes Entity-Runtime-only; its unused
Hive-app Tauri command is removed. Full weights remain in the runtime document,
`birth_certificate.json`, `forge_soul.json`, and the runtime soul material.

A recursive regression test rejects `weights`, `deontology`, `teleology`,
`areteology`, and `welfare` keys from Genesis session/action responses, quest
catalog, journey, Presence, and generic birth responses. A separate runtime
test proves the lease-scoped birth document still carries the hidden,
version-stamped certificate.

## 3. Longer, data-driven mentoring arc

### Catalog data

One static definition table drives badge title, hint, XP, optional activity
requirement, and unlock metadata. A second table drives bond tiers. The quest
catalog returns both tables so the frontend has no duplicate progression curve.

Creation badge XP is rebalanced so earning every creation badge reaches at most
bond level 2. Post-creation tiers extend beyond Beacon:

1. Spark
2. Kindling
3. Ember
4. Hearthfire
5. Beacon
6. Constellation
7. Legacy

Exact thresholds and milestone targets live only in the Rust catalog tables and
are asserted by tests.

### Real activity

`EntityJourney` gains defaulted monotonic counters:

- mentor messages;
- distinct conversation sessions;
- active UTC days;
- active ISO weeks;
- successful skill uses;
- confirmed memories.

Conversation/session/day/week facts are reconciled from the Entity's durable
conversation turns. The bounded 500-turn scan is replaced with full durable
summaries so old activity does not disappear.

Successful skill calls and confirmed memories are Runtime events:

- the journal queues one `skill_used` outbox record for each successful tool
  call, containing only session ID, skill ID, and tool name;
- a successful explicit non-ephemeral memory insert queues
  `memory_confirmed`, containing only memory ID, weight tier, and timestamp.

Hive stores only validated, redacted event summaries in `entity_journey_event`,
keyed by Runtime outbox `record_id`. Duplicate sync is idempotent. The table
contains no arbitrary payload and no message, reply, prompt, or memory content.
Owner and Entity IDs are cross-checked against the authenticated Runtime lease.

Journey projection merges durable conversation summaries and redacted event
counts, then awards milestones monotonically. Suggested first quests remain
copy-only; no activity is generated on the mentor's behalf.

## 4. Model setup gate

For an Entity card linked to an unfinished Genesis session while Hive reports
`needs_provider`:

- subtitle: `Connect a model to continue Genesis`;
- primary action: `Connect a model to continue`;
- click target: Provider Wizard;
- after successful connection: refresh status and restore the ordinary
  `Continue <name>'s Genesis` action.

The button does not enter Genesis while the model gate is active. New First
Signal creation remains available because static choices and durable saving are
local; the gate applies when returning through an allocated Entity card.

## Failure handling

- Unknown or mismatched creation keys fail closed and preserve the receipt.
- Interrupted session creation replays the same key and seed.
- Interrupted Entity linking reuses the receipt's Entity ID.
- Corrupt Presence, creation receipt, Genesis seed, journey, or journey event
  data is reported and never silently reset.
- Unknown Runtime outbox kinds remain accepted by the general sync path but do
  not affect progression.
- Journey projection failure prevents acknowledging a tracked progression event
  so Runtime can retry the same record ID.

## Testing

Tests cover:

- keyed reservation, concurrent replay, restart recovery, conflict, terminal
  deletion, and legacy unkeyed compatibility;
- keyed Genesis create/resume, seed mismatch, interrupted link persistence, and
  slim First Signal dispatch recovery;
- canonical Presence shape and 32 KiB write/read limits;
- no public weight or axis keys across all named routes;
- injective version-3 certificate signing with raw version-1/version-2
  verification compatibility;
- creation XP capped at level 2 and every catalog tier/milestone;
- duplicate outbox sync, owner/Entity mismatch rejection, content-free stored
  events, restart durability, and monotonic projections;
- conversations, sessions, days, weeks, skills, and memories awarding only
  their configured badges;
- model-less Genesis cards opening Provider Wizard rather than Genesis;
- complete Rust formatting, strict Clippy, workspace tests, and both split
  frontend builds/tests.

## Deferred slices

- Migration and recovery of the old standalone First Signal browser journals.
- Candidate matching for unkeyed legacy direct-creation requests.
- The old standalone animated Presence builder and separate Presence home.
- Event-table compaction or aggregate snapshots if household histories become
  large enough for measured query cost to justify them.
- Calendar/seasonal/family-wide quests and subjective progression tuning after
  UAT.
