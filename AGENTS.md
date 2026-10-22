# AGENTS.md — Active Implementation Plan for All Coding Agents

This file tracks the current plan for agents working in the Abigail repository.

## Current State (2026-03-05)

Abigail is the private Entity Coordinator and Manager for real homes and families. Its product mission is to put persistent, highly configurable, ethically governed AI Entities into the hands of everyday people through one easy install and one obvious app launch. The user (mentor/family head) creates individual Entities that the family actually interacts with. Abigail handles coordination, memory, skills, and security in the background.

- Abigail Hive is the only place where provider/model management should live.
- Abigail Hive should remain visible and usable even while an Entity is open.
- The active implementation split is two app roots: `hive-app` for control-plane work and `entity-runtime-app` for the chat/runtime surface, but the family-facing product must install and launch as one `Abigail` app.
- The full installer lane must package Hive, Entity Runtime, `hive-daemon`, and `entity-daemon` together so non-technical families never handle separate binaries.
- `beta` is the permanent primary iteration branch. Agents should target PRs and automation at `beta` for UAT builds, then promote validated beta changes to `main`.
- Every push to `beta` must produce a properly tagged beta installer prerelease for UAT using the `vX.Y.Z-beta.N` tag pattern.
- Mentor chat monitor preprompt flow is in place and out-of-band monitors remain non-blocking.
- DevOps Forge worker is active and subscribed to `topic.skill.forge.request`.
- Forge pipeline writes sandbox-gated artifacts to `skills/dynamic/`, updates `skills/registry.toml`, and publishes `topic.skill.forge.response`.

## Active Plan (Family-First Priorities)

### Entity Genesis (updated 2026-10-22)

- Entity creation now runs through Hive's resumable Genesis workspace and authenticated local Genesis API, using the selected Hive model with tools disabled. AI and Markdown output are reviewable proposals, never silent canonical edits.
- Slim First Signal creates a canonical UUID before dispatch, journals only version 1 plus that key and the reviewed canonical creation seed, and retries the exact keyed Genesis session. Hive's durable receipt permanently binds that key and seed fingerprint to at most one Entity UUID; conflicts fail closed and deleted/missing Entities make the receipt terminally unavailable. Do not revive standalone candidate matching or reuse creation keys.
- Canonical schema v1, sources, conversation, revisions, media metadata and generation jobs belong to the Hive store. Local household capability authentication is not multi-account or child-role authorization.
- Approved narrative is included in signed birth documents; fictional lore and memories never modify operational permissions. Media failures do not prevent activation.
- Presence uses only canonical `PresenceCard`/`PresenceChoice`, stays outside `docs/` and prompts, grants no authority, and has a 32 KiB read/write limit. The old standalone First Signal journals, animated Presence builder, separate Presence home, and unkeyed candidate recovery remain deferred.
- New birth certificates use scoring version `triangle-ethic-v1`; their explicit, injective v3 payload binds every displayed field and all hidden weights while raw v1/v2 verification remains compatible. User-facing Genesis, quest, journey, Presence, catalog, generic birth and legacy desktop Soul Forge DTOs must remain weight/axis-free. Full certificates and weights are restricted to local signed/runtime files and the matching Entity Runtime lease.
- The canonical Rust catalog owns all 24 badge requirements/XP/unlocks and the seven bond floors: Spark 0, Kindling 80, Ember 180, Hearthfire 350, Beacon 600, Constellation 950, Legacy 1400. All ten creation badges total 120 XP and derive from confirmed Genesis state; First Light specifically requires a confirmed Genesis awakening/Entity link and is never awarded by generic birth. Long-term activity badges derive only from durable Runtime facts: messages, sessions, UTC days, ISO weeks, successful skill uses and confirmed memories. Quests never send messages or run skills.
- Runtime reserves durable outbox capacity before tool-capable turns and explicit lasting-memory inserts. Committed assistant metadata produces one content-free aggregate skill event; explicit-memory metadata produces one content-free memory event. Startup/heartbeat recovery uses deterministic IDs; persisted acknowledgements prevent requeue but not local ledger repair, and reconciliation failure stops heartbeat/sync. Unmatched reservations move to persisted, non-capacity quarantine only after 24 hours and never create activity. Hive journey rows remain idempotent, owner/Entity-validated closed metadata only; never copy messages, replies, prompts, model output, tool arguments/results, memory content or arbitrary Runtime payload into journey storage.
- An unfinished Genesis card in `needs_provider` shows subtitle `Connect a model to continue Genesis` and button `Connect a model to continue`; the button opens Provider Wizard and must not enter Genesis until a successful model connection refreshes state. First Signal remains locally available.
- Portraits are versioned and films require the approved portrait reference. Media work requires explicit cost confirmation and server-side budget checks. Do not activate the retired Sora API or claim fixture tests prove live model/media/device acceptance.
- Genesis retention is opt-in per creation record. Expiry rechecks ownership and inactivity under the same action lock, deletes sources/transcript/media, and preserves awakened Entities and real conversations. Canonical Markdown exports restore deterministically; mixed freeform sections yield additional reviewable proposals.
- Track design, provider configuration and exact acceptance evidence in `docs/ENTITY_GENESIS.md`. Preserve existing installer and CLI safeguards.
- Fresh 2026-10-22 final-review verification passed formatting; strict all-target Clippy for `entity-daemon`, `hive-core`, `hive-daemon`, `abigail-persistence`, `abigail-app` and `soul-forge`; 291 affected Rust tests with 8 ignored; Hive UI build plus 92 tests; legacy Tauri UI build plus 54 tests; and focused ethics/journey privacy scans. This is scoped source verification, not installed-desktop or live-provider acceptance.
- Implementation validation: scoped backend 117 passed (44 Genesis included; eight pre-existing ignored), Hive/Runtime UI 36/7 passed, strict Clippy and all four embedded builds passed. Unsigned installer `dist/entity-genesis/Abigail-windows-x64-setup.exe` passed actual four-payload verification and 14 installed acceptance stages including live Codex with no fixture fallback. These results are separate from tagged desktop acceptance.
- Tagged `v0.0.76-beta.130` (commit `39c334a833fb1762c7d00c939e23d05c18c3ba34`) passed all 10 installed desktop acceptance stages on 2026-10-17: valid signed installer and all four payloads, real Codex connection, Quick Genesis creation of Aster, approval of 11 proposals, exact draft save/resume, signed activation and awakening, real chat, exact history after Runtime reopen and full Hive/Entity process restart, and correct context recall. Hive stayed available while Aster was open. The final five-turn history preserved the original three turns exactly; approved identity, Genesis choices, selected Codex and validated birth signature persisted.
- Desktop acceptance used an isolated profile and unchanged release binaries. The initial run was invalidated because PowerShell retained empty test variables and enabled ephemeral storage; the successful rerun removed environment keys with `ProcessStartInfo.Environment.Remove`. See `docs/ENTITY_GENESIS.md` for artifact identity and evidence. Other Genesis modes, paid media, microphone/speech hardware, multi-account and child-role behavior remain outside this desktop acceptance.

### Unsigned MVP implementation (2026-10-02)

- The acceptance path is install -> launch Hive -> connect a real model -> create/name/purpose/sign an Entity -> chat -> close/reopen -> restore conversation.
- `scripts/build-mvp-windows.ps1` bundles standalone Hive/Runtime frontends and both daemons into one unsigned NSIS installer. `scripts/verify-mvp-windows.ps1` checks the installed executable payload.
- `-Channel preview` (compiled through `ABIGAIL_BUILD_PROFILE=preview`) builds **Abigail Preview**, which installs beside stable with its own app identifiers, data root, vault credential, Documents folder, and Hive ports. Channel differences live only in `crates/abigail-channel` and the `tauri.preview.conf.json` overlays; the manual Preview Candidate workflow produces unsigned artifacts, never releases. See `docs/MVP_WINDOWS.md`.
- Hive is the single embedded-store process. Entity runtimes use the Hive persistence API with a matching runtime lease; Entity conversation and job scopes remain separate inside the shared store.
- Every Hive route except `/health`, the adoption challenge, and bootstrap lease issuance requires the per-install `hive-control.token` or a runtime lease. Entity daemons exchange their bootstrap for a lease before any other Hive call. Hive app and `hive-cli` adopt a daemon only after its HMAC proof of the token; Hive and Entity daemons reject a non-loopback `Host`. Runtime-facing routes must be listed in the policy table in `crates/hive-daemon/src/access.rs`; unlisted routes accept only the desktop token.
- No built-in local inference is claimed without a configured real model. Local servers, cloud providers, and installed official Claude/Codex/Grok CLIs are setup paths. A desktop installation alone does not prove CLI inference; Gemini CLI remains unsupported.
- CLI discovery is passive. Explicit selection in Hive requires a successful, nonempty real model probe before saving; Codex/Grok may be offered for checking when authentication is unknown. Official CLIs own account authentication and tokens; Abigail does not extract or forward OAuth tokens.
- Genesis Quest (stages, stat cards, badges, Presence, celebration) is presentation only: it creates proposals, never canon; `presence.json` stays outside `docs/` and out of prompts; Genesis-allocated Entities are born only through Genesis. See `docs/ENTITY_GENESIS.md`.
- Dangerous tools remain behind explicit backend confirmation. Installed CLI inference is chat-only: native tools, MCP, hooks, and extensions are disabled or rejected when they cannot be isolated. Do not mutate the user's CLI configuration to make a connection work.
- `scripts/tests/run-mvp-acceptance.ps1` exercises concurrent Entities, streaming, scoped persistence, and restart durability using a synthetic model fixture. Real-model and desktop acceptance are recorded separately in `docs/MVP_WINDOWS.md`.
- The prior unsigned Windows MVP installer passed actual installation, all four payload checks, 12 installed contracts, 10 real local-model stages, and desktop chat/history reopening. Its backend plus enabled daemon integration tests passed 501 tests; frontend suites passed 15. These are the prior MVP results, not totals for the installed-CLI expansion. They do not cover household accounts, child-specific roles, generated native code, or live cloud/CLI inference.
- Installed-CLI validation is tracked separately in `docs/MVP_WINDOWS.md`: Codex `0.153.4` passed 11 installed real-inference stages without an API key, including live streaming, conversation context, selected-provider traces without fallback, and exact history after Entity/Hive restarts. Grok Build `0.2.93` passed four negative authentication checks without saving a default; real Grok inference awaits user sign-in. The installed daemon contract passed all 12 stages. The final unsigned installer and all four installed payloads are verified; its daemons and Runtime GUI match the accepted payload hashes, with a refreshed Hive GUI for small-window scrolling.
- The expanded backend regression passed 546 distinct tests (517 unit plus 29 integration), with 10 ignored. Hive and Entity frontend suites passed 17 tests. The latest capabilities run replaces its earlier count rather than being added again. Do not equate Grok's negative authentication acceptance with real inference.
- Final installed desktop acceptance passed eight stages: restore Codex selection, reject Grok sign-in failure while preserving Codex, create/birth Nova, chat through the bundled Runtime while Hive stays available, and restore the exact prompt/reply after closing and reopening. Hive and Nova were left open in that isolated demo profile. Claude's installed negative connection check passed four stages without saving a default; a scalar native diagnostic confirmed HTTP 401/authentication required. Claude needs user `claude auth login`; real Claude inference remains unaccepted.
- Existing-account Grok must not call ACP `authenticate`, including `cached_token`: that RPC can fall back to interactive login. Consume only the cached account established by `initialize` and `session/new`; missing or expired authentication must fail without starting sign-in. Do not mark a provider connected on discovery, account metadata, or protocol initialization alone.
- Respect [Claude Code product-use conditions](https://code.claude.com/docs/en/legal-and-compliance#can-customers-offer-claude-code-in-their-products). Current [Codex app-server account authentication](https://learn.chatgpt.com/docs/app-server#auth-endpoints) is for local/open-source integrations; commercial or hosted products need the applicable approved Sign in with ChatGPT path.

1. Keep the always-open Hive shell and Hive-owned model management stable and test-backed.
2. Use `beta` as the primary integration and UAT branch; keep `main` as the promoted stable release branch.
3. Keep the one-step installer and one-app launch path repeatable, tested, and aligned with the split runtime architecture.
4. Harden Forge envelope validation and failure telemetry (keep it invisible and safe for the user).
5. Expand end-to-end coverage for forge request/response and watcher hot-reload.
6. Keep memory/safety/id-superego observers out-of-band (non-blocking chat path) so the family experience stays smooth.
7. Keep the unsigned stabilization lane free of installer upgrade-preserve logic, updater assumptions, and Windows signing dependencies.

## Definition of Done for Next Phase

- Forge request envelope accepts code + markdown and persists deterministically.
- Windows installer installs Abigail as one app and bundles the split Hive/Runtime daemons as internal resources.
- Merges to `beta` automatically create tagged beta prereleases for UAT; stable releases come only from promoted `main`.
- Superego and sandbox gates prevent unsafe mutations while staying invisible to the user.
- Registry update reliably triggers watcher-based hot-reload.
- End-to-end coverage validates success, blocked, and error fallback behavior.
- Legacy compatibility paths that conflict with the current dev UX are removed instead of preserved.

## Documentation Sync

When changing routing or monitor behavior, update:
- `README.md` (user-facing family story)
- `CLAUDE.md` and `AGENTS.md` (agent constitution / active plan files)

**Remember the Mission**: Abigail coordinates the Entities that families actually talk to. Every change must make the experience warmer, simpler, and more powerful for real homes.
