# Entity Genesis implementation design

Design recorded before implementation, 2026-10-17.

## Architecture and scope

Genesis lives in the existing React Hive app and Rust Hive daemon. It reuses the
configured Hive model, the single Hive-owned SurrealKV store, and the signed birth
ceremony. Entity Runtime stays a separate internal process; the installed product
remains one Abigail app. No new cloud database, account system, or agent framework
is introduced.

The canonical, versioned definition separates narrative identity (including
explicitly fictional memories) from permissions and runtime configuration.
Schema-validated field changes cannot grant operational authority. AI output and
Markdown imports are proposals requiring review; existing approved facts survive
until the mentor explicitly accepts replacement. Definition revisions support undo.
Sessions preserve input modes, conversation, sources, decisions, and progress.

Hive stores sessions and media job metadata durably. Genesis HTTP endpoints require
a private local desktop capability token; ownership is the current local household,
matching Abigail's existing single-user installation. This is not a claim of
multi-household or child-role account support. Browser development requires an
explicit token. Assets are fetched through authenticated, ownership-checked routes,
never from arbitrary file paths or public links.

## Workstreams

1. Domain schema, state machine, import/review/export, history and durable store.
2. Hive conversation orchestration, authenticated API, activation and runtime identity.
3. Responsive conversation/constellation/studio UI, voice capability detection and
   consent, accessible fallback animation, save/resume and review.
4. Private portrait versions and durable, bounded, confirmed media jobs with a
   pluggable provider boundary. Rendering failure never prevents activation.
5. Integration verification, runtime regression checks and user documentation.

## Provider decisions

The model already selected in Hive is authoritative. Genesis will not silently
switch the user's provider, claim an unverified model works, or change CLI settings.
The public API model identifier documented for Astra is `gpt-6-astra`; an “Astra
Ultra” API identifier has not been verified. The five creative responsibilities
are prompt modules in one tools-disabled model request.

OpenAI's current deprecation notice says the Sora 2 models and Videos API were
removed on 2026-09-24, with no recommended replacement. Therefore Sora must not be
offered as an active provider. Portraits use GPT Image 2. The built-in Runway
Gen-4 Turbo adapter produces 5 or 10 second films using the selected, approved
portrait as its reference. A free animated emergence is available without a paid
provider. Provider access and visual quality are separate from fixture acceptance.
See [media setup, limits, costs and provider evidence](GENESIS_MEDIA.md).

Sources checked 2026-10-17:
- https://developers.openai.com/api/docs/models/gpt-6-astra
- https://developers.openai.com/api/docs/deprecations
- https://developers.openai.com/api/docs/guides/video-generation

## Acceptance strategy

Check domain invariants, malformed and conflicting imports, ownership, concurrency,
undo and restart persistence. Exercise API creation through activation and actual
runtime identity loading with an isolated synthetic model. Test UI mode switching,
review, import and error recovery, then build both existing frontends. Test media
workers using deterministic transports for success, failure, cancellation and
idempotency without billing an account. Live voice/device and paid media results
must be reported separately; unsupported speech engines must show a text fallback.

The supplied attachment ends at “A user can switch input modes without” in its
acceptance section. All preceding complete requirements inform this design; no
missing acceptance text is assumed.

## Using Genesis

Connect a real conversation model in Hive, then choose **Create an Entity**.
Guided, Quick and Deep Genesis share the same draft as Voice, Markdown and Hybrid.
Describe the Entity or choose a suggested answer, review the resulting proposals,
and explicitly accept the details to keep. A replacement of an approved fact
requires a separate confirmation. The six essence scores describe which fields
are filled; they do not measure character quality or prevent awakening.

Markdown accepts `.md` and `.markdown` up to 256 KiB. Structured headings work
offline; freeform biographies are interpreted by the selected model. Originals
and source references remain available until deleted. Unknown sections are kept.
Exports contain readable narrative plus a typed canonical block for reliable
reimport. Fictional memories are labeled created lore and never imported as real
conversation history.

Voice requires explicit session consent. Windows uses native local dictation when
browser speech recognition is unavailable. Browser speech may use a remote speech
service. Abigail stores submitted transcripts, not microphone recordings. Spoken
commands can approve suggestions, review, awaken, skip a question, or save and
exit; typed input remains available if speech cannot start.

Optional portraits and films use credentials entered under **Genesis media
connections** in Hive. A CLI conversation connection does not confer image or
video API access. OpenAI and Runway credentials stay in Hive's existing vault.
Generation, retries and regeneration each require a displayed cost confirmation.
Select a portrait before requesting a film, then explicitly compare and approve
the film's identity. Background work survives leaving the workspace; Hive reports
completion or a recoverable failure.

Confirm a name and purpose, resolve conflicting proposals, and awaken whenever
ready, including while a render is pending. Activation reuses the existing signed
birth ceremony and stores the approved narrative with the Entity. Its personalized
first greeting is persisted once in real chat history. Later intentional revisions
apply when the mentor chooses to evolve the Entity; an already running Runtime is
stopped so its next launch loads the new signed identity.

Genesis owns birth for the Entities it allocates. Until awakening, Hive refuses
generic setup (`POST /v1/entities/:id/birth`) and opening for that Entity, and the
Entity card routes back to its Genesis record. Afterwards generic birth still
refuses to replace the signed Genesis story. If saving `definition.entity_id`
is interrupted after allocation, the durable creation receipt retains the same
Entity UUID. Session views derive a presentation-only effective link from that
receipt so the card still routes to Genesis; an exact retry saves the canonical
link without allocating a replacement.

## Genesis Quest and owner decisions

The following four product decisions were implemented on 2026-10-22.

### 1. Canonical Presence, slim First Signal and durable creation

The **Quest** tab and progress rail present creation as seven stages:
Reservation, Shaping the Soul, Ethics, Instincts, Presence, Birth and Awakening.
Only Reservation (a confirmed name and purpose) is required. The quest is
presentation and UX only:

- **Temperament stat cards** create an ordinary pending personality proposal.
  The mentor still accepts or rejects it. A card fills description and style
  only where they are empty.
- **Ethics** records answers to the three Soul Forge trials. When all three are
  answered, awakening forges the soul from them (`birth_path: forge`);
  otherwise it uses the balanced quickstart soul. Trial weight effects are not
  displayed, so the mentor chooses on values.
- **Instincts** shows the constitutional `instincts.md` template read-only.
- **Presence** uses the canonical `PresenceCard` and `PresenceChoice` types. It
  stores a cosmetic `presence.json` beside the Entity, outside the `docs/`
  folder that runtimes read for the system prompt. Display name, purpose and
  boundaries are copied from the approved definition; requests carry only
  enumerated temperament, palette, crest and motion choices. Presence grants
  nothing, never enters prompts and is symmetrically limited to 32 KiB on read
  and write. Missing cards produce an empty result; malformed or oversized
  cards fail without replacement.

The slim First Signal collects name, purpose and those canonical Presence
choices, then uses Quick mode to enter the existing Genesis workspace. Before
dispatch, the Hive frontend journals exactly version 1, a canonical UUID
`creation_key`, and the canonical `GenesisCreationSeed`; it serializes access
with a Web Lock or process-local fallback. The journal clears only when Hive
returns that same key as the durable Genesis session ID. Failures, malformed
journals and response-ID mismatches preserve the record for an exact retry.

Hive permanently binds each creation key to one request fingerprint and at most
one Entity UUID in the defaulted global creation-receipt ledger. Reservation
precedes file creation; replay is idempotent across concurrency and restart;
seed/name conflicts fail closed. Completion and registry publication are
atomic. Deleting or losing the receipt's Entity makes the key terminally
`unavailable`, never reusable. Genesis activation and optional keyed generic
creation share this receipt mechanism, and
`GET /v1/entity-creations/:creation_key` reports `absent`, `reserved`,
`complete` or `unavailable` under Hive's control-token policy.

### 2. Hidden, versioned ethics

New signed birth certificates carry the exact immutable scoring version
`triangle-ethic-v1`. Their explicit v3 signature payload uses canonical
length-prefixed fields and binds every displayed certificate field, the scoring
version, and the exact bits of all four hidden ethics weights. Genuine raw v1
documents with no `scoring_version` key and prior scoring-stamped raw v2
documents continue to verify against their byte-stable payloads. Explicit v3,
raw v2, and raw v1 dispatch is unambiguous; removing or injecting version fields
cannot downgrade or upgrade a signature.

User-facing Genesis, quest, journey, Presence, catalog and generic birth
responses expose only the weight-free `BirthCertificateSummary`: archetype,
epithet, sigil, birth path, issue timestamp, scoring version and verification
result. Ethics weights, axis values and scoring structures are absent from
those surfaces. The full certificate remains only in local signed/runtime
artifacts and the matching Entity Runtime's lease-scoped birth document;
desktop control-token access cannot retrieve it.

The legacy desktop Soul Forge also sanitizes at its Rust command boundary.
Scenario DTOs contain only IDs, titles, descriptions and weight-free choices;
the crystallization DTO contains only archetype, soul hash and sigil. Hidden
weights remain inside `soul-forge` and never enter frontend state or completion
callbacks.

### 3. Data-driven mentoring journey

Badge titles, hints, XP, activity requirements, cosmetic unlocks and bond tiers
come from one Rust catalog, which the frontend renders without a duplicate
curve. Creation awards at most 120 XP, so completing all ten creation badges
reaches Kindling but remains below Ember.

| Bond level | XP floor |
| --- | ---: |
| Spark | 0 |
| Kindling | 80 |
| Ember | 180 |
| Hearthfire | 350 |
| Beacon | 600 |
| Constellation | 950 |
| Legacy | 1400 |

The exact badge curve is:

| Badge | Durable requirement | XP |
| --- | --- | ---: |
| First Spark | Begin creation | 5 |
| A Name Is Given | Confirm a name | 10 |
| A Reason to Be | Confirm a purpose | 10 |
| Soul Shaper | Confirm identity plus values or three traits | 15 |
| Storyteller | Confirm lore and a created memory | 15 |
| Trial Walker | Answer all three Soul Forge trials | 15 |
| Keeper of Instincts | Read the constitutional instincts | 10 |
| A Face for the Hearth | Choose Presence | 10 |
| Curator | Review an imported Markdown document | 10 |
| First Light | Confirm Genesis awakening and its Entity link | 20 |
| First Words | 1 mentor message | 15 |
| Getting Acquainted | 5 mentor messages | 25 |
| Kindred | 25 mentor messages | 40 |
| Hearth Regular | 100 mentor messages | 80 |
| Trusted Companion | 500 mentor messages | 180 |
| Returning Companion | 5 conversation sessions | 50 |
| Seasoned Companion | 25 conversation sessions | 120 |
| Steady Rhythm | 7 active UTC days | 80 |
| Hearth Season | 4 active ISO weeks | 120 |
| Constellation Keeper | 12 active ISO weeks | 220 |
| Skill Explorer | 5 successful skill uses | 80 |
| Skill Weaver | 25 successful skill uses | 160 |
| Memory Keeper | 3 confirmed lasting memories | 80 |
| Living Archive | 15 confirmed lasting memories | 180 |

Creation badges are monotonic and derive from confirmed Genesis state, never
pending proposals. In particular, First Light comes only from a confirmed
Genesis awakening/link; generic birth does not award it. Long-term activity badges and their monotonic counters
derive only from durable Runtime facts. Conversation turns reconcile the full
durable history for mentor messages, distinct sessions, active UTC days and
active ISO weeks. Committed successful tool calls and explicit non-ephemeral
memory inserts add content-free fallback facts and queue `skill_used` or
`memory_confirmed` outbox events.

Hive validates lease owner and Entity IDs before storing recognized events in
the closed, schematized `entity_journey_event` table, keyed idempotently by
Runtime `record_id`. Its complete allowlist is `record_id`, `owner_id`,
`entity_id`, `kind`, `occurred_at`, and optional `session_id`, `skill_id`,
`tool_name`, `memory_id`, `memory_weight`. It never stores arbitrary Runtime
payloads, messages, replies, prompts, model output, tool arguments/results or
memory content. Unknown event kinds remain valid ordinary outbox records but
do not create or advance a journey. Projection merges durable Entity facts and
event counts by per-counter maximum.

Quest progress is saved in the defaulted Genesis `quest` field. Each Entity's
durable counters and badges live in Hive-owned `entity_journey`, so they outlive
an expired Genesis record. Suggested first quests remain copy only: no quest
sends a message or executes a skill automatically.

### 4. Model setup gate

An Entity card linked to unfinished Genesis while Hive reports
`needs_provider` shows the subtitle `Connect a model to continue Genesis` and
the primary-action button `Connect a model to continue`. That button opens
Provider Wizard and cannot enter Genesis. A successful connection refreshes
Hive state and restores `Continue <name>'s Genesis`. Local First Signal
creation remains available; already-born Runtime cards retain their existing
provider-required gate.

### Deferred standalone recovery slices

The old standalone First Signal application was not imported. Deferred work is
limited to migration/recovery of its old browser journals, candidate matching
for unkeyed legacy direct-creation requests, and its animated Presence builder
and separate Presence home. These are not compatibility promises for the
current Hive-first flow.

## Local authorization and retention

Genesis is scoped to the existing local household profile. Like every Hive route,
its HTTP routes require the per-install Hive control token (`hive-control.token` in
the data root); the native app retrieves it through Tauri once the daemon has proved
it holds the same token. Browser development needs `VITE_HIVE_DAEMON_URL` and
`VITE_HIVE_CONTROL_TOKEN` for an isolated loopback daemon. Never embed a personal
token in committed files or URLs. This is local profile protection, not a
multi-user account system.

Drafts have optimistic revisions and exclusive per-draft actions. At most two model
requests run concurrently, with 20 requests per minute. Limits include 100 sessions,
20 source documents per session, 500 messages, 500 proposals and 200 definition
revision snapshots. Session conversation text is capped at 16 KiB per message.
Original source excerpts sent to a provider are treated as untrusted narrative;
the model gets no tools, and schema validation rejects permission changes.

Sources, transcripts, generated media and entire Genesis records have explicit
deletion controls. Deleting a Genesis record also deletes its generated visuals
and clears their permanent pointers, while preserving its already awakened Entity
and actual chat history. Minimal media accounting tombstones remain until retention
policy can expire them without bypassing the daily budget. No automatic deletion
of personal history is enabled by default.

Each Genesis record can optionally expire after 1–3650 inactive days. The default,
**Keep until I delete it**, also applies to records saved before this setting
existed. Inactivity runs from the last saved change (`updated_at`), not the last
time a record was viewed. Changing the retention setting starts a fresh period.
Hive checks on startup and approximately once per minute while running. An expired
record loses its creation transcript, uploaded sources and generated visuals;
its awakened Entity and actual chat history remain. The worker skips records with
an action in progress and rechecks the latest policy and timestamp before deletion,
so a concurrent edit or disabled policy protects the record. Failed deletions are
retried without logging narrative content.

## Staged enhancements

The initial implementation includes a living-portrait toggle, character seeds,
versioned alternatives, a creation chronicle, intentional Entity evolution and the
Genesis Quest.
Relationship visualization and dedicated voice casting remain later enhancements,
as requested in the specification's staged-innovation section. Relationships and
voice descriptions are already editable canonical fields.

## Final-review source verification — 2026-10-22

Fresh affected verification on `cursor/genesis-quest-gamification-4871`
passed:

- `cargo fmt --all -- --check`;
- strict all-target Clippy for `entity-daemon`, `hive-core`, `hive-daemon`,
  `abigail-app` and `soul-forge`;
- affected Rust suites for those five packages: 269 tests passed and 8
  pre-existing daemon cluster tests were ignored;
- `cd hive-app/src-ui && npm test && npm run build`: production build plus
  92 tests across 18 files;
- `cd tauri-app/src-ui && npm test && npm run build`: production build plus
  54 tests across 14 files.

The ethics-key scan found no occurrence in the tracked Hive reveal and only a
weight-rejection assertion in the legacy Tauri frontend. Rust presentation
commands mention axis keys only in tests that reject leakage; calculation
remains internal. The journey producer scan confirms that successful
`skill_used` events are staged atomically by Ego before success publication,
the journal does not enqueue a duplicate, and explicit memory events contain
only their allowed IDs and weight class. None adds content to
`entity_journey_event`.

This is source, build and automated fixture verification only. No installer,
installed WebView, live provider, microphone, paid media or subjective visual
acceptance was run or claimed for this change.

## Verification — 2026-10-17

The final backend regression for `hive-core`, `hive-daemon` and `entity-daemon`
passed **117 tests**, including **44 Genesis tests**
(13 core and 31 daemon). Eight pre-existing `daemon_e2e` tests remain ignored;
the enabled HTTP and subprocess integration suites ran with `ABIGAIL_CI_MODE=1`
and `ABIGAIL_DAEMON_INTEGRATION=1`. Counts are overlapping scopes, not additive.
The complete output is `target/genesis-final-regression.log`. This scoped run does
not replace the earlier whole-workspace MVP totals in `MVP_WINDOWS.md`.

Hive passed **36 frontend tests across 11 files**. Entity Runtime passed **7 tests
across 3 files**. Both production builds passed. The final Hive entry is
`index-DJt--cv7.js`; packaged frontends use embedded assets with inline styles.
`cargo fmt --all -- --check` passed, and Clippy passed with `-D warnings` for
`hive-core`, both daemons, and both native app packages.

All four executables built with `--locked --features tauri/custom-protocol`.
The unsigned debug installer is
`dist/entity-genesis/Abigail-windows-x64-setup.exe` (version `0.0.1`). Its build
manifest is `dist/entity-genesis/mvp-build.json`; SHA-256 is
`57AC56328469BFB940B750B400D472244DA934DE4F780EF35F4E9DE28B4E4C9E`.
Actual installation to `target/manual-test/mvp-installed-e4c76f3d` passed all four
payload hashes, Windows runtime dependency checks and both daemon loader checks.
Evidence: `target/manual-test/mvp-installed-e4c76f3d-verification/installed-payload.json`.

The **installed daemons passed 14/14 acceptance stages**: 13 synthetic stages and
one live Codex stage. Synthetic stages covered authenticated access, proposal
approval, mode/composer persistence, stale-edit rejection, malformed model output,
permission injection, structured/freeform/mixed Markdown, preserved partial import
failure, exact canonical reimport, 256 KiB upload boundaries, undo, deletion,
retention settings, signed activation, Runtime identity loading, media gating, and
exact history after Hive/Entity restart. The live stage performed a real Codex
connection probe, produced schema-valid proposals for Mira, required approval, and
awakened her. An assertion confirmed that the local synthetic model received no
director requests during that stage. No API key was injected or CLI configuration
modified. Report:
`target/manual-test/genesis-acceptance/2026-10-17T17-09-12-445Z-7710d18b/report.json`.

Reproduce the installed acceptance with:

```powershell
node scripts/tests/genesis-acceptance.mjs --binary-dir target/manual-test/mvp-installed-e4c76f3d/resources --live-provider codex-cli
```

Omit `--live-provider` for fixture-only acceptance. The script creates isolated
profiles and cleans up only daemons whose executable and data-directory ownership
match that run. Earlier intermediate runs are superseded by this final report.

An isolated browser run verified proposal approval, all six mode choices,
Hybrid-mode persistence, saving/resuming an exact unsent message, awakening
without paid media, Markdown conflict review, selective import, and applying
identity refinements. The narrow view had no horizontal overflow, and the desktop
view exposed all three workspace regions. No browser console errors were observed.
Evidence: `target/manual-test/genesis-acceptance/browser-validation.json`,
`genesis-mobile.jpg`, and `genesis-desktop.jpg` in the same directory. Later import
error and retention controls are covered by the final UI regression suite.

Live microphone capture, installed speech-engine behavior, billed image/video
generation, and subjective portrait/film consistency have **not** been accepted.
Media provider fixtures verify contracts and lifecycle behavior, not visual quality.
Browser acceptance does not substitute for installed WebView interaction.

### Tagged beta.130 installed desktop acceptance

**Passed all 10 stages on 2026-10-17 at 23:23 UTC** using the actual installed
Hive and Entity Runtime windows and a real, already-authenticated Codex connection.
This accepts the **Quick Genesis** desktop path on
[v0.0.76-beta.130](https://github.com/jbcupps/abigail/releases/tag/v0.0.76-beta.130),
commit `39c334a833fb1762c7d00c939e23d05c18c3ba34`.

The downloaded `Abigail-windows-x64-setup.exe` had SHA-256
`478D10ADFECA66DF344C4537ADD625B35F14B7BA5099F0DE7D0D974C2FA80AC5`.
Authenticode validation returned **Valid**, publisher James B Cupps. Actual
installation to `target/manual-test/beta130-installed` passed all four payload
hashes against the release manifest, Windows runtime dependency checks and both
daemon loader checks. The shipped binaries were not modified for acceptance.

| Stage | Verified result |
| --- | --- |
| 1. Install tagged beta | Signed installer and all four bundled payloads verified. |
| 2. Launch Hive | Installed Hive opened in a fresh isolated profile. |
| 3. Connect a real model | Hive verified and saved the existing Codex connection with a real response. |
| 4. Create an Entity | Quick Genesis proposed Aster's identity through the selected model. |
| 5. Review, save and resume | All 11 proposals explicitly accepted; definition, proposals and three discovery messages restored exactly after save/exit/resume. |
| 6. Activate | Signed birth validated; canonical definition matched the saved identity; awakening appeared once in Runtime chat. |
| 7. Chat | Real Codex reply recorded the adventure name, mascot and a five-minute activity while Hive remained available. |
| 8. Close and reopen Runtime | A new Entity daemon restored the same Entity/session and exact three ordered chat turns. |
| 9. Restart all app processes | New Hive and Entity processes restored identity, selected Codex, Genesis choices/transcript, validated birth signature and exact chat history from disk. |
| 10. Recall after restart | Aster recalled the earlier name, mascot and activity without those details being repeated in the question. |

Aster's Entity UUID is `bbc1dcd7-0e7f-4d13-a773-5233239a5d18`; Genesis session
`250becf7-6100-430a-a4f0-691ec5e0fee3` remained Quick/ready at revision 42.
The chat session was `genesis-awakening`. History checks compared ordered
role/content pairs, not just message counts. After the full restart, Aster
correctly recalled **Lantern Orchard 130**, a **teal fox**, and examining a leaf
for two minutes, sharing observations for two, then drawing it for one. The final
five-turn history retained the original awakening/prompt/reply as an exact prefix.

The full restart closed both GUIs normally, then stopped only path-validated owned
daemons after chat completed; Hive has no graceful shutdown endpoint. Zero
installed app processes remained before relaunch. Data, Documents, LocalAppData
and Roaming paths were isolated from the normal family profile. Official CLI
authentication and configuration were unchanged. Runtime status, routing and
healthy provider records established `codex-cli` use; no exact underlying model
version or per-turn diagnostic trace capture is claimed.

The first profile was excluded from acceptance: PowerShell 7.6.5 / .NET 10 retained
present-but-empty variables after `SetEnvironmentVariable(name, $null, 'Process')`,
inadvertently enabling ephemeral test storage. The successful full rerun removed
`ABIGAIL_CI_MODE` and `ABIGAIL_DAEMON_INTEGRATION` with
`ProcessStartInfo.Environment.Remove` and asserted key absence before launch.
No product change was required.

Local evidence is retained under `target/manual-test/genesis-desktop-beta130/`
(these generated artifacts are not committed): `acceptance-report.md`, the
`corrected-profile/` UI transcripts/screenshots and launch/restart records, and
`read-only-snapshots/CORRECTED-ACCEPTANCE-SUMMARY.md` with exact API comparisons.
The installed payload report is
`target/manual-test/beta130-installed-verification/installed-payload.json`.

Other Genesis modes, paid portraits/films, microphone and installed speech-engine
behavior, multi-account authorization and child-role behavior were outside this
desktop run. These 10 stages are separate from the earlier backend, browser and
installed-daemon totals. Desktop acceptance belongs to beta.130 specifically;
a later installer build or prerelease verification does not by itself repeat it.
