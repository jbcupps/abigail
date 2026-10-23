# Task 6 Report — Seasonal and Family Quests

## Status

Implemented on `cursor/genesis-quest-continuation-5d0b`.

The long-arc catalog contains exactly two copy-only quests:

1. `seasonal_shared_stories`: 12 current-quarter mentor messages with the
   requested Entity.
2. `family_constellation`: 30 current-quarter mentor messages contributed
   across at least two distinct active, born, non-Hive Entities in the local
   owner scope.

No route sends a message, executes a skill, or exposes a quest action.

## TDD Evidence

Tests were added and run before production implementation.

Initial red results:

- `cargo test -p hive-core genesis_quest` failed to compile because
  `LONG_ARC_QUEST_DEFINITIONS` and `LongArcQuestScope` did not exist.
- `cargo test -p hive-daemon long_arc` failed to compile because the UTC
  quarter, seasonal summary, progress-view, and owner-scoped aggregation
  functions did not exist.
- `npx vitest run src/components/GenesisQuest.test.tsx` ran eight tests and
  failed the new card test because no accessible `Seasonal quests` region was
  rendered.

Those failing contracts were committed separately as `f7b4c7e`.

An intermediate production build then found stale typed `QuestCatalog` mocks
in two neighboring UI suites. The mocks were updated with an empty
`long_arc_quests` catalog and committed separately as `1f3eba9`; no production
interface was weakened to accommodate old fixtures.

## Core Catalog and Public Shapes

`hive-core` now owns:

- `LongArcQuestScope`
- `LongArcQuestDefinition`
- `LONG_ARC_QUEST_DEFINITIONS`
- `LongArcQuestView`

The canonical definition array has length two and uses only
`JourneyMetric::MentorMessages`. Definitions contain display copy, scope,
target, and minimum Entity count. They contain no prompt, action, conversation
content, model output, or Soul Forge scoring fields.

Journey progress views contain only:

- quest and season identifiers;
- a display season label;
- explicit RFC3339 UTC start and exclusive-end bounds;
- numeric progress and participating Entity count; and
- completion.

## UTC Season Semantics

Seasons are UTC calendar quarters with stable IDs in `YYYY-QN` form.

For `2026-Q4`:

- `starts_at` is `2026-10-01T00:00:00Z`;
- `ends_at_exclusive` is `2027-01-01T00:00:00Z`;
- a turn exactly at the start counts;
- a turn exactly at the exclusive end does not count and begins `2027-Q1`.

Tests cover the year boundary and verify that next-quarter progress resets
instead of carrying the prior quarter's messages.

## Progress Derivation and Scoping

Entity progress is derived from distinct durable user-turn IDs whose timestamps
fall within `[starts_at, ends_at_exclusive)`. Assistant turns and duplicate user
turn IDs do not count.

Family aggregation:

- accepts only the daemon's current local owner scope;
- iterates the local registered identity scope;
- excludes Abigail Hive;
- excludes Entities that are not born/active;
- opens each eligible Entity's own scoped durable history;
- sums mentor-message counts with overflow checking; and
- requires at least two Entities with current-season contributions before the
  family quest can complete.

If an eligible Entity history is malformed or inaccessible, calculation fails
closed with the fixed message `Seasonal journey activity is unavailable.` It
does not return a partial family total, storage path, Entity name, or
conversation content.

The two exact catalog entries use only the mentor-message metric. Redacted
Runtime event rows carry skill/memory activity, not mentor messages, so they
are not treated as message evidence. Tests compact an old redacted skill event
and confirm both current-season message quests remain at zero. Current-season
conversation filtering therefore cannot be affected by old compaction
snapshots.

## Daemon and UI

The Genesis quest catalog now includes `long_arc_quests`.

Each Entity journey response now includes content-free `long_arc_quests`
progress for the current UTC quarter. Existing recursive route tests continue
to verify that user-facing Genesis, catalog, birth, Presence, and journey
responses contain no hidden ethical weights.

After awakening, the Hive Quest view renders compact accessible cards showing:

- title;
- Entity or Family scope;
- season label;
- progress;
- family participation where applicable; and
- completion.

The card region contains no button or other action control. The existing
`Nothing is sent for you. Copy a prompt, or say whatever feels right.` copy
remains visible. The component is top-level, stateless, and uses native
`section`, `article`, and `progress` semantics; no additional hooks or client
state were introduced.

## Documentation

`docs/ENTITY_GENESIS.md` now documents:

- the exact two-quest catalog;
- UTC quarter IDs and exclusive end semantics;
- owner/Entity scoping;
- failure behavior;
- compaction isolation;
- content/scoring exclusions; and
- the no-action/no-auto-send UI contract.

## Verification

Final verification passed:

- `cargo fmt --all -- --check`
- `cargo clippy -p hive-core -p hive-daemon --all-targets -- -D warnings`
- `cargo test -p hive-core genesis_quest` — 15 passed
- `cargo test -p hive-daemon long_arc` — 6 passed
- `npx vitest run src/components/GenesisQuest.test.tsx` — 8 passed
- `cargo test -p hive-core` — 39 passed
- `cargo test -p hive-daemon` — 163 unit, 8 integration, 1 startup passed
- `npm test` in `hive-app/src-ui` — 130 passed
- `npm run build` in `hive-app/src-ui` — TypeScript and Vite build passed

## Commits

- `f7b4c7e` — `test(journey): define seasonal quest contracts`
- `7c0a9e8` — `feat(journey): add seasonal and family quests`
- `1f3eba9` — `test(hive-ui): update quest catalog fixtures`

No commits were pushed, as requested.

## Concerns

None open. The intentional fail-closed behavior means one inaccessible active
Entity history makes long-arc progress temporarily unavailable rather than
showing a potentially incorrect partial family total.
