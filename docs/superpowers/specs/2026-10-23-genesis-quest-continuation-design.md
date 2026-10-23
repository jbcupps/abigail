# Genesis Quest Continuation Design

## Goal

Complete the six ordered Genesis Quest follow-up slices on top of PR #305 while
keeping its Presence schema, ethics secrecy, content-free progression, and
post-creation arc authoritative.

## Non-negotiable boundaries

- `PresenceCard` and `PresenceChoice` in `hive-core::genesis_quest` remain the
  only persisted Presence schema.
- Soul Forge scoring values and dimensions never cross an internal calculation
  boundary into UI state, HTTP responses, logs, diagnostics, or quest data.
- Journey and quest activity carries identifiers, timestamps, counters, and
  closed enums only. It never copies a message, reply, prompt, model output,
  tool argument/result, or memory content.
- A migration never creates an Entity, retries an ambiguous old creation, or
  treats an exact-name candidate as proof. The mentor selects an existing
  candidate before any migration write.
- Quests suggest activities and report durable progress. They never send text,
  invoke a skill, or mutate model/provider/tool authority.
- Every migration and compaction operation is replay-safe across interruption.
- Windows PowerShell compatibility is preserved for the owner's split-stack
  launcher.

## Considered approaches

### 1. Server-owned migration and projection (selected)

Strictly decode the two historical First Signal journal formats in the Hive
frontend. Resolve a keyed completed receipt or present exact-name, post-baseline
unkeyed candidates for explicit mentor selection. A desktop-only Hive endpoint
then atomically converges the selected Entity onto the canonical Presence and
journey models. A durable migration ledger permanently binds the old attempt to
one Entity and one sanitized import fingerprint.

This approach preserves candidate ambiguity, survives a response lost after a
successful write, and gives the server authority over canonical composition.

### 2. Client-only conversion to the current launch journal

This was rejected because old direct-creation receipts have a different
fingerprint contract and unkeyed requests have no replay-safe key. Replaying
them through current Genesis could conflict or allocate a second Entity.

### 3. Automatic exact-name adoption

This was rejected because names are not identities. Concurrent or older
same-name Entities make automatic adoption unsafe.

## 1. Legacy First Signal migration

The frontend decoder recognizes only:

- session-storage `abigail.first-signal.creation.v1`; and
- local-storage `abigail.first-signal.creation.v2`.

It validates the historical closed profile and attempt shapes without importing
legacy runtime code. Reads never mutate storage. Malformed or conflicting bytes
remain untouched and block a new creation until manually recovered.

For keyed journals, the UI performs the existing exact receipt lookup and offers
only a completed Entity. It never resumes a reserved or absent old direct
creation. For unkeyed journals, it lists registered non-Hive Entities whose
exact name was not present in the journal baseline. Every result remains labelled
as a candidate and needs a mentor click.

The migration request contains an opaque attempt key, original registered name,
purpose, boundaries, and canonical Presence choice. The legacy three-value
temperament and palette enums map losslessly; crest defaults to `orbit` and
motion to `calm`. Hive verifies the selected registered Entity and original
name, composes a canonical `PresenceCard`, and adds only the applicable
presentation badges (`FirstSpark`, `Named`, `Purposeful`, `PresenceChosen`) to
the durable `EntityJourney`.

The migration ledger stores owner, selected Entity, a digest of the closed
request, state, and timestamps—never profile prose. The same request converges
after partial failure; a different Entity or payload for the same old attempt
fails closed. Canonical Presence that a mentor changed after a completed import
is never overwritten by a replay.

## 2. Quarantine diagnostics

Entity Runtime exposes a read-only diagnostic projection of quarantined journey
reservations:

```text
{ record_id, kind, quarantined_at_utc, reason }
```

The nested reservation, Entity identifier, soul reference, payload digest,
session identifier, and all activity payload data stay private. The reason is
fixed backend copy. The existing outbox status gains only a count, while a
separate endpoint returns the sanitized rows for local mentor diagnostics.

Recursive tests reject ethics-scoring keys and content-bearing keys from the
serialized diagnostic response.

## 3. Soul Forge trial balance

An internal audit enforces two properties per scenario:

1. no answer Pareto-dominates another answer; and
2. every answer contributes the same total calibration influence.

The built-in trial calibration is adjusted where necessary to satisfy both
properties. This changes only new internal forge calculations; no scoring data
is added to presentation types or docs. User documentation states the product
principle in value-oriented language: each answer expresses a defensible
trade-off and there is no stat-optimal answer.

## 4. Animated Runtime Presence crest

The Entity Runtime obtains its own canonical Presence through an Entity-scoped
Hive request proxied by Entity daemon. Hive access policy permits a Runtime lease
to read only that Runtime's Presence; writes remain desktop-only.

The Runtime header renders the saved crest and palette with the card's chosen
motion. A reduced-motion hook replaces animated motion with `still` and responds
to operating-system preference changes. CSS also disables animation inside the
reduced-motion media query as defense in depth. Presence failure never blocks
conversation restoration.

## 5. Journey event compaction

Compaction replaces many old event rows with one owner-scoped snapshot record.
The snapshot stores only:

- successful skill-use and confirmed-memory totals;
- the exact record IDs and canonical digests required for collision-safe
  idempotency; and
- compaction timestamps.

It contains no event payload or prose. The snapshot is written before covered
rows are deleted. Projection ignores still-present rows already represented by
the snapshot, so interruption cannot double count. Incoming retries compare
against compacted ID/digest entries: exact retries are acknowledged and
conflicting reuse fails closed.

Automatic compaction runs only above a conservative row threshold and only for
events older than the current UTC season. Current-season rows remain available
for seasonal quest calculation. Tests cover pre/post projection equivalence,
partial deletion replay, exact retry, collision rejection, owner isolation, and
recursive leak scanning.

## 6. Seasonal and family quests

The long arc adds a small canonical quest catalog separate from badges:

- an Entity seasonal quest based on current-season durable activity; and
- a household quest that aggregates the same content-free metrics across at
  least two Entities.

The current season is a UTC calendar quarter with a stable ID and explicit start
and end timestamps. Progress is derived from durable conversation timestamps and
redacted journey events within that window. Family aggregation remains scoped to
the local owner and reports only counts.

Journey responses include quest definitions and progress views. The Hive Quest
tab renders compact progress cards after awakening and repeats that nothing is
sent automatically. Catalog and route leak tests cover the new shapes.

## Windows split-stack correction

`Start-Process -Environment` is unavailable in Windows PowerShell 5.1. The
launcher will use a compatibility helper that temporarily overlays the current
process environment while spawning each desktop child, then restores the exact
prior values. A static contract test prevents reintroduction of the unsupported
parameter and verifies both desktop apps receive their required variables.

## Verification

Each slice starts with a failing focused test, followed by the minimum
implementation and its focused suite. Before completion:

- format and lint the Rust workspace;
- run the full Rust workspace suite excluding the legacy aggregate Tauri app, as
  in PR #305;
- run tests and production builds for Hive, Entity Runtime, and legacy Tauri UI;
- run the legacy command-surface contract;
- run the Windows launcher contract; and
- scan every added public response recursively for scoring and narrative leaks.

