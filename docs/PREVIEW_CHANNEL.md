# Preview channel

`beta` is the integration and preview source branch. `main` remains stable;
feature work stays in separate dev branches and worktrees. There is no third
release channel. Preview releases keep `vX.Y.Z-beta.N` tags, and each published
installer is signed, timestamped, source-bound, and immutable.

The single user entry point is
**https://jbcupps.github.io/abigail/preview/**.
It selects one accepted beta release from `docs/preview/current.json`; it does
not select the most recent prerelease automatically. Until an exact build
passes this contract, the descriptor remains `unavailable` and the page offers
no preview installer. This scaffolding does not establish that a preview is
ready.

Updates are manual and opt-in. Show the full beta tag, source commit, installer
SHA-256, release notes, known gaps, and the test checklist. The installer embeds
the numeric application version, so that version alone cannot distinguish
successive `beta.N` builds. Do not enable an automatic updater or reuse the
stable `/releases/latest/download/` route for preview.

## Application and packaging contract

The separate preview overlays, selected product/compiled identities and proposed
launcher interface are documented in [Preview Windows packaging](PREVIEW_PACKAGING.md).
Packaging metadata alone does not implement application isolation.

Stable and preview must coexist without either replacing, discovering, writing
to, or stopping the other's installation/profile/processes.

| Area | Required behavior |
| --- | --- |
| Installer | A distinct preview product and uninstall identity, install directory, shortcut, and launcher. `/D` alone is not isolation: NSIS can replace the registered prior product before honoring that directory. Inspect the generated installer recipe and test its running-process behavior. |
| Compiled applications | Embed distinct preview Hive and Runtime identifiers during compilation. A bundle-time filename change is insufficient. Preserve one obvious launch icon per channel. |
| Paths | Explicitly scope data, shared database, identities, configuration, recovery documents, archives, backup discovery, logs, and runtime descriptors. Reject a missing/invalid preview root; never fall back to the stable Windows KnownFolder path. |
| Process discovery | Validate channel, profile, and live process-instance identity before adoption. Reject cross-channel descriptors and inherited URLs. Startup failure or port contention must not fall back to another Hive. |
| Child processes | Pass the correct endpoint, profile, and packaged executable paths explicitly. Closing/restarting either channel must affect only its owned children. Test stale descriptors, overrides, simultaneous launch, crashes, and orphan handling. |
| Continuing Entity | Retain the same Entity UUID, identity, history, settings, and durable state across accepted preview upgrades and reopening. Do not recreate this Entity for each test. |
| Creation tests | Use separate disposable synthetic profiles/Entities. A reset of these fixtures must never reset the continuing preview profile. |
| Migrations | Check schema compatibility before writing. Refuse a newer unsupported schema without modifying it. Test supported migrations on a clone, with a cold snapshot available. |
| Backup/restore | Quiesce the complete profile; include the shared `memory.db` directory, identities, configuration, and all persistent external paths. Restore to a new isolated fixture and verify the same UUID/history/settings. Per-Entity export is not a complete profile backup. |

Binary rollback does **not** undo data migration. Returning to an older preview
requires a separately preserved compatible full-profile snapshot. Stable keeps
its own unchanged profile. Never point stable at a migrated preview profile.
No test or release procedure may delete or alter a user's real profile.

## Required acceptance evidence

Use synthetic fixtures in a disposable Windows environment. Keep a stable
baseline and a continuing preview profile distinct from fresh-creation
fixtures. Record the exact source commit, beta tag, canonical installer name,
size and SHA-256, publisher certificate thumbprint, harness revision, commands,
and assertion results. A copied checklist or a single `verified: true` field
does not establish acceptance.

Every stage below must pass for the exact candidate:

| Stage ID | Required assertions |
| --- | --- |
| `separate-installer-identity` | Generated installer and compiled app identities are distinct; files, registrations, shortcuts, and process checks are channel scoped. |
| `stable-preview-coexistence` | Install, upgrade, and uninstall preview with stable present/active. Stable files, profile, registrations, processes, and conversation remain unchanged. Test both launch orders. |
| `profile-path-isolation` | All persistent paths stay in the chosen preview fixture, including archive creation and backup discovery. Invalid preview roots fail closed. |
| `daemon-ownership-isolation` | Concurrent launches, wrong/stale descriptors, inherited endpoints, occupied ports, crashes, and close/reopen never adopt or stop stable. |
| `fresh-creation-fixture` | Installed payload checks and the existing 12-stage daemon contract pass; fresh Entity creation happens only in disposable fixtures. |
| `ongoing-entity-upgrade` | Upgrade a prior accepted synthetic preview profile while retaining the same UUID, conversation, settings, and state. For the first preview, record an explicit seeded baseline and its source/hash; do not invent a previous accepted build. |
| `ongoing-entity-reopen` | Close/restart Runtime and Hive, then verify exact prior history and the same Entity identity. |
| `full-cold-backup-restore` | Quiesce and snapshot the complete profile, restore into a new fixture, then verify UUID/history/settings and shared-store state. |
| `newer-schema-refusal` | An unsupported newer profile is rejected before writes, with byte-level preservation assertions. |
| `return-to-stable` | Stable remains usable with its unchanged profile. A preview rollback uses a separately restored compatible snapshot, never an in-place downgrade. |

The existing fresh-profile synthetic daemon contract is useful but does not
prove installer coexistence, GUI discovery isolation, migration, or full
restore. Model-provider credentials are unnecessary for these synthetic gates;
real-model acceptance must be labelled separately.

## Selecting a preview

1. Coordinate the exact integration source with the application owner. Reconcile
   selected beta features and the signing/provenance safeguards without a blind
   main/beta merge. Build and sign once as a nonpublishing candidate, run the
   same-commit required CI gate and all acceptance stages above against those
   final installer bytes, and retain the approved artifact and SHA-256.
   Inspect the final built installer, not merely source settings.
2. Publish a unique signed beta tag through a reviewed publication step that
   reuses those exact accepted bytes. Dispatching the full release workflow
   builds/signs again; do not transfer acceptance to that new installer hash.
   If the available publication path cannot reuse the accepted artifact, stop
   and coordinate artifact promotion before publishing.
   Nonpublishing artifacts have `channel=candidate` evidence/provenance and
   cannot be uploaded unchanged as a beta release. Prepare the final asset set
   with `scripts/prepare_preview_assets.ps1` on Windows:

   ```powershell
   ./scripts/prepare_preview_assets.ps1 `
     -CandidateAssetsDirectory <accepted-signed-candidate-directory> `
     -SourceCommit <exact-source-sha> `
     -Tag <same-beta-tag-as-candidate> `
     -CertificateThumbprint <reviewed-publisher-thumbprint> `
     -OutputDirectory <new-final-beta-directory>
   ```

   This nonpublishing helper validates the candidate, preserves all installer
   bytes, verifies their native signatures again, and creates only new
   beta-channel evidence/provenance. It refuses an unsigned candidate, a changed
   tag/source, a stable-form tag, or an existing destination. The candidate
   directory is unchanged. Review the final output and upload that exact set;
   do not relabel published metadata or use `--clobber`.
   `make_latest` remains false. Refuse an existing release/tag mismatch and
   asset replacement. Do not publish an unsigned beta; artifact-only unsigned
   candidates remain available with `publish_release=false`.
3. Run `scripts/verify_published_release.ps1` on Windows against the exact
   repository, tag, source, approved asset directory, and certificate thumbprint.
   It verifies the remote tag, release metadata, downloaded hashes/provenance,
   Authenticode and timestamp, and that stable remains latest. Do not execute
   the downloaded installer on a personal profile. Record the stable baseline's
   exact tag/source/installer hash and the unchanged latest-download bytes before
   and after publication; the preview CLI also checks the pinned stable release
   ID, source and installer metadata.
4. Assemble sanitized, source/installer-bound compatibility and public-download
   evidence from actual successful checks. Do not publish credentials, personal
   paths, model conversations, or full workflow logs. Store those JSON reports
   at `docs/preview/evidence/<sha256>.json` in a reviewed repository commit.
   The `.gitattributes` rule preserves exact report bytes instead of converting
   line endings. Verify the committed raw report hash, then reference its exact
   commit-pinned raw GitHub URL. Public verification
   happens after release creation, so do not append it to finalized release
   assets or change their original provenance.
5. Prepare a `ready` descriptor with the pinned reports, their exact content and
   hashes, full release identity, known gaps, and user checklist. Run the
   preview validator's online check, which must resolve the selected release
   and tag, match public assets/provenance/signature evidence, and compare the
   public report bytes. Offline synthetic tests do not authorize promotion.
   Run `node scripts/preview_channel.mjs validate docs/preview/current.json`.
   It is read-only: it does not edit the descriptor, publish, sign, or install.
6. Promote only the page/evidence/descriptor through a reviewed documentation
   change to `main`. The repository's existing Pages source is `main:/docs`.
   First pin the evidence commit, then commit the pointer referencing it.
   Verify the rendered live route and exact download target after Pages builds.
   Announce preview readiness only then.

The page validates the curated descriptor and bound report contents in the
browser. It does not run Windows Authenticode or claim a fresh live check.
Revalidate the public build when advancing the pointer. If a selected release
is withdrawn or its evidence is invalid, set the descriptor to `unavailable`;
retain prior immutable releases and evidence for diagnosis.

Ordinary `GITHUB_TOKEN` pushes do not trigger branch-based Pages builds.
Use the existing reviewed docs publication path; do not silently change Pages
settings, add credentials, or broaden token permissions. Historical-workflow
differences may also prevent automated tag creation: resolve the source/workflow
coordination explicitly instead of rebuilding, relabelling, or replacing assets.

## Initial implementation status

The initial selector is unavailable. Same-account isolation and the compatibility
harness still require application-owner implementation and exact-source
acceptance before any preview is selected. Existing beta releases are not
grandfathered into this contract.
The helper prepares candidate-to-beta assets but does not create a tag/release
or advance the pointer. Coordinate the publication source and exact staged
artifact with the release owner. This scaffolding does not invoke the historical
one-off stable recovery workflow for previews.

Source review of the stable signing baseline identified these handoff points:

- `crates/abigail-core/src/config.rs`: Windows KnownFolder data defaults and
  automatic configuration migration/writeback.
- `crates/entity-daemon/src/main.rs` and
  `crates/abigail-memory/src/archive.rs`: automatic archive export uses the real
  Documents known folder through `ArchiveExporter::with_defaults`.
- `crates/abigail-memory/src/backup_import.rs`: backup scanning uses real
  `Documents/Abigail/backups`.
- `hive-app/src/lib.rs`: descriptor health-only adoption, inherited
  `ABIGAIL_HIVE_URL`, fixed-port fallback, and per-process Runtime tracking.
- `hive-app/tauri.conf.json`, `entity-runtime-app/tauri.conf.json`,
  `scripts/stage_split_installer_resources.ps1`, and
  `scripts/build_signed_installer.ps1`: compiled and installer identities.

These are reviewed gaps, not claims that the application fixes have shipped.

References:
[GitHub Pages publishing source](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site),
[GitHub release API](https://docs.github.com/en/rest/releases/releases#create-a-release),
[Microsoft SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation).
A valid signature and timestamp do not guarantee SmartScreen reputation for a
new preview artifact.
