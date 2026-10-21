# AGENTS.md — Active Implementation Plan for All Coding Agents

This file tracks the current plan for agents working in the Abigail repository.

## Current State (2026-10-20)

Abigail is the private Entity Coordinator and Manager for real homes and families. Its product mission is to put persistent, highly configurable, ethically governed AI Entities into the hands of everyday people through one easy install and one obvious app launch. The user (mentor/family head) creates individual Entities that the family actually interacts with. Abigail handles coordination, memory, skills, and security in the background.

- Abigail Hive is the only place where provider/model management should live.
- Abigail Hive should remain visible and usable even while an Entity is open.
- The active implementation split is two app roots: `hive-app` for control-plane work and `entity-runtime-app` for the chat/runtime surface, but the family-facing product must install and launch as one `Abigail` app.
- The full installer lane must package Hive, Entity Runtime, `hive-daemon`, and `entity-daemon` together so non-technical families never handle separate binaries.
- `beta` is the permanent primary iteration branch. Agents should target PRs and automation at `beta` for UAT builds, then promote validated beta changes to `main`.
- Beta installer prereleases use the `vX.Y.Z-beta.N` tag pattern and must pass signing and release gates. The fixed `/preview/` route selects only an explicitly accepted build; a beta push does not advance that pointer automatically.
- Mentor chat monitor preprompt flow is in place and out-of-band monitors remain non-blocking.
- DevOps Forge worker is active and subscribed to `topic.skill.forge.request`.
- Forge pipeline writes sandbox-gated artifacts to `skills/dynamic/`, updates `skills/registry.toml`, and publishes `topic.skill.forge.response`.

## Active Plan (Family-First Priorities)

### Provider readiness (2026-10-21)

`dev/provider-readiness-ux` starts at tested product `a18249a3b622e8b34d0f20ec1f0e24fc0dc7183d`. Keep installed-tool presence, authentication hints, explicit connection checks and saved model selection distinct. Preview discovery resolves executable presence only; it must not execute CLI metadata/auth commands or enable the existing account adapters. Preserve API/local routes and route missing-provider conversation attempts to setup without touching saved identity or draft. See [contract and evidence](documents/PROVIDER_READINESS_UX.md). The active hands-on preview, other worktrees and paused Store files remain untouched. No live account grants, provider calls, restart, signing, installation or beta/main publication is part of this update.

### First Signal / Genesis integration (2026-10-20)

- The [bounded correctness review](documents/GENESIS_FIRST_SIGNAL_REVIEW.md) fixes legacy-birth/premature-open routing for Genesis reservations and canonical-to-presentation text boundaries. Preserve old receipts and names. Its four vault compiler diagnostics and three unused supervisor helpers have received behavior-preserving cleanup. Run final exact-commit checks before claiming source readiness; see [preview preparation](documents/GENESIS_PREVIEW_READINESS.md) for the build-only next step and actual hosted prerequisites. No automatic CI runs for this dev push. Do not confuse the old manually pinned harness with validation of this source or resume paused security work.

- Architecture A is selected: First Signal presentation with Genesis as the canonical backend for newly reviewed identity. The bounded branch `dev/genesis-first-signal-integration` merges `dev/first-conversation-return` at `e11d108dc3b44a1afca0a7f61a54ec47bb6eb737` with beta `ea92effd31f554f74bb5e109deaa42876ec4f06f`. This supersedes the earlier instruction to keep beta separate for comparison, without changing the dev-only publication boundary.
- Quick creation approves name/purpose without inference. Optional discovery uses the same durable Genesis draft; original keyed input is immutable and retries must not overwrite later reviewed edits. Entity allocation uses the existing durable receipt before the Genesis link is saved. Unavailable receipts never authorize a replacement UUID.
- Old visual briefs and unfinished First Signal receipts stay presentation-only and use their existing recovery flow. Existing Genesis links, Entity IDs, real history and selected sessions survive ordinary return. Appearance preferences do not become operational permissions or silently become signed narrative.
- Genesis JSON/assets use verified native ownership headers, and native capability reads use the verified profile root. Preserve checked runtime shutdown, profile isolation, whole-profile recovery and beta's static Windows runtime gates. Do not treat prior component/installer evidence as acceptance of this integrated source.
- See [contract](documents/GENESIS_FIRST_SIGNAL_CONTRACT.md) and [validation](documents/GENESIS_FIRST_SIGNAL_VALIDATION.md). Only development edits/pushes are authorized in this task. Leave the separate Store worktree, installed app/data, credentials and paused remediation untouched. No main/beta push, release signing, installer execution, publication or purchases.

### Entity Genesis (2026-10-17)

- Entity creation now runs through Hive's resumable Genesis workspace and authenticated local Genesis API, using the selected Hive model with tools disabled. AI and Markdown output are reviewable proposals, never silent canonical edits.
- Canonical schema v1, sources, conversation, revisions, media metadata and generation jobs belong to the Hive store. Local household capability authentication is not multi-account or child-role authorization.
- Approved narrative is included in signed birth documents; fictional lore and memories never modify operational permissions. Media failures do not prevent activation.
- Portraits are versioned and films require the approved portrait reference. Media work requires explicit cost confirmation and server-side budget checks. Do not activate the retired Sora API or claim fixture tests prove live model/media/device acceptance.
- Genesis retention is opt-in per creation record. Expiry rechecks ownership and inactivity under the same action lock, deletes sources/transcript/media, and preserves awakened Entities and real conversations. Canonical Markdown exports restore deterministically; mixed freeform sections yield additional reviewable proposals.
- Track design, provider configuration and exact acceptance evidence in `docs/ENTITY_GENESIS.md`. Preserve existing installer and CLI safeguards.
- Implementation validation: scoped backend 117 passed (44 Genesis included; eight pre-existing ignored), Hive/Runtime UI 36/7 passed, strict Clippy and all four embedded builds passed. Unsigned installer `dist/entity-genesis/Abigail-windows-x64-setup.exe` passed actual four-payload verification and 14 installed acceptance stages including live Codex with no fixture fallback. These results are separate from tagged desktop acceptance.
- Tagged `v0.0.76-beta.130` (commit `39c334a833fb1762c7d00c939e23d05c18c3ba34`) passed all 10 installed desktop acceptance stages on 2026-10-17: valid signed installer and all four payloads, real Codex connection, Quick Genesis creation of Aster, approval of 11 proposals, exact draft save/resume, signed activation and awakening, real chat, exact history after Runtime reopen and full Hive/Entity process restart, and correct context recall. Hive stayed available while Aster was open. The final five-turn history preserved the original three turns exactly; approved identity, Genesis choices, selected Codex and validated birth signature persisted.
- Desktop acceptance used an isolated profile and unchanged release binaries. The initial run was invalidated because PowerShell retained empty test variables and enabled ephemeral storage; the successful rerun removed environment keys with `ProcessStartInfo.Environment.Remove`. See `docs/ENTITY_GENESIS.md` for artifact identity and evidence. Other Genesis modes, paid media, microphone/speech hardware, multi-account and child-role behavior remain outside this desktop acceptance.

### Unsigned MVP implementation (2026-10-02)

- The acceptance path is install -> launch Hive -> connect a real model -> create/name/purpose/sign an Entity -> chat -> close/reopen -> restore conversation.
- `scripts/build-mvp-windows.ps1` bundles standalone Hive/Runtime frontends and both daemons into one unsigned NSIS installer. `scripts/verify-mvp-windows.ps1` checks the installed executable payload.
- Hive is the single embedded-store process. Entity runtimes use the Hive persistence API with a matching runtime lease; Entity conversation and job scopes remain separate inside the shared store.
- No built-in local inference is claimed without a configured real model. Local servers, cloud providers, and installed official Claude/Codex/Grok CLIs are setup paths. A desktop installation alone does not prove CLI inference; Gemini CLI remains unsupported.
- CLI discovery is passive. Explicit selection in Hive requires a successful, nonempty real model probe before saving; Codex/Grok may be offered for checking when authentication is unknown. Official CLIs own account authentication and tokens; Abigail does not extract or forward OAuth tokens.
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
7. Keep unsigned artifact-only stabilization independent of signing and updater credentials. Published stable and beta installers require signing.
8. Preserve an ongoing preview Entity in its own channel/profile across accepted upgrades. Separate creation tests use disposable fixtures. Require full cold-backup, migration, restore, path/process isolation, and coexistence acceptance before selecting a preview; see `docs/PREVIEW_CHANNEL.md`.

## Definition of Done for Next Phase

- Forge request envelope accepts code + markdown and persists deterministically.
- Windows installer installs Abigail as one app and bundles the split Hive/Runtime daemons as internal resources.
- Merges to `beta` request tagged beta prereleases subject to signing and release gates; stable releases come only from promoted `main`. Selecting a preview additionally requires the compatibility acceptance contract.
- Superego and sandbox gates prevent unsafe mutations while staying invisible to the user.
- Registry update reliably triggers watcher-based hot-reload.
- End-to-end coverage validates success, blocked, and error fallback behavior.
- Arbitrary development compatibility is not promised, but ongoing preview Entity preservation is an acceptance gate. Binary rollback cannot substitute for restoring a compatible complete profile snapshot.

## Documentation Sync

### Combined preview development (2026-10-20)

- The selected isolated integration branch is `dev/preview-combined-0.0.76`. Its published inputs are app `ca6a7527da8787be6b5315e13e4fc228287ce15d` plus its cold-recovery descendant `4558fe68d9812998eb53a94ba157970af552f923` from `dev/first-signal-integration-0.0.76`, and packaging `de65e7e47fa35c7b742421274db5e1fac4720f8d` from `dev/preview-packaging-0.0.76`, including release scaffold `6b246d3ab07322a899106d1198bfafba3a7b2309`. These are based on stable main `17b636b9e5f10b812deb287ad2c6637188991ec2`. Preserve the original development branches, main conversation restoration, model setup and signing/provenance safeguards.
- Combining, validating, committing and pushing this dev candidate is authorized separately from normal beta iteration. Main/beta merges, signing, tags, release/Pages publication, installer execution, real credentials and new access grants remain outside this task. Stable 0.0.76 remains the available release. Beta `ea92effd31f554f74bb5e109deaa42876ec4f06f` Genesis, static-runtime and installer-lock changes remain separate for explicit later comparison; do not merge beta blindly.
- Hive creation and inline Entity interaction share the active Runtime presence source. Provider/model management stays in Hive.
- Creation uses a durable client key in the existing Hive registry, exact lookup, and same-key replay. Unknown transport outcomes never authorize a fresh key.
- `/v1/entities/:id/presence` stores presentation metadata only. Do not interpret imported brief text as authority or silently inject it into runtime instructions.
- The browser design preview and synthetic HTTP fixture are explicitly distinct from live runtime/model activity. See `documents/FIRST_SIGNAL.md` for scope, validation, and integration gaps.
- The implemented preview contract is recorded in `documents/PREVIEW_RELEASE_ALIGNMENT.md` and `docs/PREVIEW_PACKAGING.md`. The combined candidate aligns both descriptive runtime overlays, their validator, tests and documentation with the application source. Build all four binaries with `ABIGAIL_BUILD_PROFILE=preview`; roots, UUIDs and endpoints are resolved and validated at launch, never frozen in release metadata. Reprepare source, overlay, contract and executable hashes after changes; preserve stable defaults and all signing/publication gates.
- Windows same-user/device cold snapshot/verify/restore is now included from the published application descendant. It preserves whole-profile data and UUIDs, holds process/cold leases, probes existing credentials and restores only into a fresh destination; see [PREVIEW_COLD_RECOVERY.md](documents/PREVIEW_COLD_RECOVERY.md). The offline `abigail-profile` utility is separate from the four packaged application binaries; adding it to an installer is not part of this integration.
- The application's prior 167 selected Rust / 373 frontend tests and the recovery slice's 104 distinct Rust tests / seven CLI smoke checks are historical component evidence, not new combined test totals. Three ignored recovery child fixtures are not extra passes. Current combined reruns and preparation are recorded in [PREVIEW_COMBINED_VALIDATION.md](docs/PREVIEW_COMBINED_VALIDATION.md). Real OS credentials, normal GUI reopening, combined lifecycle, Windows install/upgrade/uninstall/restore and all ten exact-installer acceptance stages remain open. Do not run a normal preview against real user data.
- Use this isolated checkout and the authorized published sources. Do not edit prior workers' worktrees or retry denied cross-worktree Git-directory inspection by another route. Do not resume blocked vulnerability remediation. See `documents/FIRST_SIGNAL_INTEGRATION.md` and `documents/DEVELOPMENT.md` for the original work and current development publication boundary.

### Hosted acceptance harness preparation (2026-10-20)

- Prepare, review, test and push only the development source for the manual [hosted acceptance harness](docs/PREVIEW_HOSTED_ACCEPTANCE.md). Product source is pinned to `f7c7848d25726480b253ec1ca637c3ffa5ea3c77`; the separately reviewed harness/dispatch commit must be recorded too. A future hosted build produces new bytes and evidence; local preparation does not transfer acceptance to them.
- The proposed `.github/workflows/preview-candidate.yml` uses a disposable GitHub-hosted Windows runner and read-only repository permissions. Current authorization does not dispatch it, register it on the default branch, sign, execute an installer, or perform DPAPI/credential operations. Do not alter the existing release workflow, main/beta, release tags, Pages, the unavailable preview selector, or access grants.
- A later approval must cover this exact workflow and reviewed harness commit, fresh stable and preview fixture identities/DPAPI material, isolated install/uninstall and process teardown, and one signing pass using existing SSL.com secret references. Both manual approval booleans default to false; they record explicit approval rather than creating it. Registering a new manual workflow on the default branch is a separate authorization, not an implied consequence of a dev push.
- The stable comparison pins actual `v0.0.76` source `e3ad51181da1ea11bded24ca0c39e2dcfe5fa944`, release assets and installed payload hashes in `scripts/preview-candidate-baseline.json`. Keep all fixture data beneath the unique hosted `RUNNER_TEMP` run root, use only a loopback synthetic model, and upload only the allowlisted signed candidate bytes and public hashes/reports. Do not upload fixture profiles, secrets or raw logs. Every exact-installer stage and normal GUI/WebView acceptance remains pending until its own evidence exists.

When changing routing or monitor behavior, update:
- `README.md` (user-facing family story)
- `CLAUDE.md` and `AGENTS.md` (agent constitution / active plan files)

**Remember the Mission**: Abigail coordinates the Entities that families actually talk to. Every change must make the experience warmer, simpler, and more powerful for real homes.
