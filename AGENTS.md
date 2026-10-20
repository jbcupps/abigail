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
- Final installed desktop acceptance passed eight stages: restore Codex selection, reject Grok sign-in failure while preserving Codex, create/birth Nova, chat through the bundled Runtime while Hive stays available, and restore the exact prompt/reply after closing and reopening. Hive and Nova remain open in the isolated demo profile. Claude's installed negative connection check passed four stages without saving a default; a scalar native diagnostic confirmed HTTP 401/authentication required. Claude needs user `claude auth login`; real Claude inference remains unaccepted.
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

- The selected isolated integration branch is `dev/preview-combined-0.0.76`. Its published inputs are app `ca6a7527da8787be6b5315e13e4fc228287ce15d` from `dev/first-signal-integration-0.0.76` and packaging `de65e7e47fa35c7b742421274db5e1fac4720f8d` from `dev/preview-packaging-0.0.76`, including release scaffold `6b246d3ab07322a899106d1198bfafba3a7b2309`. Both are based on stable main `17b636b9e5f10b812deb287ad2c6637188991ec2`. Preserve the original development branches, main conversation restoration, model setup and signing/provenance safeguards.
- Combining, validating, committing and pushing this dev candidate is authorized separately from normal beta iteration. Main/beta merges, signing, tags, release/Pages publication, installer execution, real credentials and new access grants remain outside this task. Stable 0.0.76 remains the available release. Beta `ea92effd31f554f74bb5e109deaa42876ec4f06f` Genesis, static-runtime and installer-lock changes remain separate for explicit later comparison; do not merge beta blindly.
- Hive creation and inline Entity interaction share the active Runtime presence source. Provider/model management stays in Hive.
- Creation uses a durable client key in the existing Hive registry, exact lookup, and same-key replay. Unknown transport outcomes never authorize a fresh key.
- `/v1/entities/:id/presence` stores presentation metadata only. Do not interpret imported brief text as authority or silently inject it into runtime instructions.
- The browser design preview and synthetic HTTP fixture are explicitly distinct from live runtime/model activity. See `documents/FIRST_SIGNAL.md` for scope, validation, and integration gaps.
- The implemented preview contract is recorded in `documents/PREVIEW_RELEASE_ALIGNMENT.md` and `docs/PREVIEW_PACKAGING.md`. The combined candidate aligns both descriptive runtime overlays, their validator, tests and documentation with the application source. Build all four binaries with `ABIGAIL_BUILD_PROFILE=preview`; roots, UUIDs and endpoints are resolved and validated at launch, never frozen in release metadata. Reprepare source, overlay, contract and executable hashes after changes; preserve stable defaults and all signing/publication gates.
- The application's prior 167 selected Rust and 373 frontend tests are component evidence, not combined build, lifecycle or installed acceptance. Complete-profile cold snapshot/restore is assigned separately to the app worker and is not implemented by this integration. Real OS credentials, combined lifecycle, Windows install/upgrade/uninstall/restore and all ten exact-installer acceptance stages remain open. Do not run a normal preview against real user data.
- Use this isolated checkout and the authorized published sources. Do not edit prior workers' worktrees or retry denied cross-worktree Git-directory inspection by another route. Do not resume blocked vulnerability remediation. See `documents/FIRST_SIGNAL_INTEGRATION.md` and `documents/DEVELOPMENT.md` for the original work and current development publication boundary.

When changing routing or monitor behavior, update:
- `README.md` (user-facing family story)
- `CLAUDE.md` and `AGENTS.md` (agent constitution / active plan files)

**Remember the Mission**: Abigail coordinates the Entities that families actually talk to. Every change must make the experience warmer, simpler, and more powerful for real homes.
