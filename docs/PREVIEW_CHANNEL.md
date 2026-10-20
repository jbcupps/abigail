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

The preview overlays, selected product/compiled identities and implemented
launcher interface are documented in [Preview Windows packaging](PREVIEW_PACKAGING.md).
The selected dev integration and exact source inputs are recorded in
[application/release alignment](../documents/PREVIEW_RELEASE_ALIGNMENT.md).
The application implements the profile and process guards; packaging metadata
describes that contract. Neither source review nor component tests establish
installed-candidate acceptance.

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

## Combined development status

The selector remains unavailable. `dev/preview-combined-0.0.76` combines
published application `ca6a7527da8787be6b5315e13e4fc228287ce15d` with packaging
`de65e7e47fa35c7b742421274db5e1fac4720f8d`, including release scaffold
`6b246d3ab07322a899106d1198bfafba3a7b2309`. The shared stable-main base is
`17b636b9e5f10b812deb287ad2c6637188991ec2`. This dev source selection does not
promote main/beta, sign or publish a release, or accept an installer. Stable
0.0.76 remains available. Existing beta releases are not grandfathered into
this contract.

Application source implements validated preview context, scoped paths and
credentials, schema refusal, verified daemon identities and pinned transport.
Both shells' overlays describe the same runtime interface. The stage builder
compiles all four binaries with `ABIGAIL_BUILD_PROFILE=preview`; each shell uses
its own compiled overlay. Roots, UUIDs, endpoints and ports remain runtime
values. Changed overlays or contract require fresh preparation and hashes.
Prior application and packaging test totals are separate component evidence,
not proof of the combined candidate's lifecycle or installation.

Complete-profile cold snapshot/restore is assigned to a separate application
slice and is not implemented by this integration. Actual combined GUI/daemon
lifecycle, real Windows credential integration, install/upgrade/uninstall,
continuing-Entity reopen/restore and all ten exact-installer stages remain open.
Use only separate synthetic fixtures for the later acceptance. Beta Genesis,
static-runtime and installer-lock changes at
`ea92effd31f554f74bb5e109deaa42876ec4f06f` remain separate for explicit later
comparison; do not merge beta blindly.

The helper prepares candidate-to-beta assets but does not create a tag/release
or advance the pointer. Coordinate the publication source and exact staged
artifact with the release owner. This scaffolding does not invoke the historical
one-off stable recovery workflow for previews.

The earlier source review of the stable signing baseline identified these
handoff points. Their current development status is:

- `crates/abigail-core/src/profile.rs` and `config.rs`: immutable validated
  preview context, independent profile marker, scoped paths and newer-schema
  refusal are implemented; stable defaults are preserved.
- `crates/entity-daemon/src/main.rs` and
  `crates/abigail-memory/src/archive.rs`: preview archive export uses the
  validated profile Documents root.
- `crates/abigail-memory/src/backup_import.rs`: preview backup scanning is
  scoped to the validated profile root; per-Entity restore does not replace
  complete-profile snapshot/restore.
- `hive-app/src/lib.rs` and `entity-runtime-app/src/lib.rs`: preview connections
  verify every process-identity field and pin routing headers. Failed native
  resolution has no endpoint/browser fallback or followed redirects. Cleanup
  requires verified acknowledgement and owned-child exit, never stale-PID
  termination.
- `hive-app/tauri.preview.conf.json`, `entity-runtime-app/tauri.preview.conf.json`,
  `scripts/stage_split_installer_resources.ps1`, and
  `scripts/build_signed_installer.ps1`: distinct preview identities and
  source/overlay/contract/executable-bound preparation are implemented while
  stable defaults and signing/publication guards remain preserved.

These are development implementation claims. None marks an exact-installer
acceptance stage passed or claims that the preview has shipped.

References:
[GitHub Pages publishing source](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site),
[GitHub release API](https://docs.github.com/en/rest/releases/releases#create-a-release),
[Microsoft SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation).
A valid signature and timestamp do not guarantee SmartScreen reputation for a
new preview artifact.
