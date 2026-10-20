# Packaging read-only checkpoint

Inspected 2026-10-20 while the application validation continued. This inspection only read files and used Git read commands with `--no-optional-locks`. It did not fetch, run tests, edit/stage files, change branches, commit, sign, install or take over packaging ownership.

The task-5 directory is `C:\Users\jbcup\Documents\Codex\2026-10-19\task-5`.

A later alignment pass read packaging files only. After the parent confirmed cross-worktree directory-entry reads were permission-denied, no cross-worktree Git check was repeated. The branch/HEAD and dirty-content observations below remain the earlier checkpoint, not a refreshed Git result.

| Checkout below task-5 | Earlier observed branch | Earlier observed HEAD |
| --- | --- | --- |
| `abigail-preview-packaging` | `dev/preview-packaging-0.0.76` | `6b246d3ab07322a899106d1198bfafba3a7b2309` |
| `abigail-preview-release` | `dev/preview-release-channel-0.0.76` | `6b246d3ab07322a899106d1198bfafba3a7b2309` |

## Independently observed

At the earlier read-only checkpoint, the packaging checkout was dirty relative to HEAD:

- Both stable `tauri.conf.json` files match HEAD after newline normalization.
- `scripts/stage_split_installer_resources.ps1`, `scripts/build_signed_installer.ps1`, `docs/PREVIEW_CHANNEL.md`, and `scripts/tests/test_signed_installer_state.ps1` differ from HEAD.
- Both `tauri.preview.conf.json` overlays, `docs/PREVIEW_PACKAGING.md`, and the preview packaging/staging/signed-capture/NSIS-hook test suites exist in the checkout but are absent from HEAD.
- `git diff --cached --name-status` returned no entries. No staged changes were observed.
- `git branch -vv` showed no upstream for the packaging branch. This is not a remote publication check.

The newer `docs/PREVIEW_PACKAGING.md` describes separate product/launcher/desktop identities, all-four-binary preview compilation, separate overlay builds, immutable preparation metadata, and a signed candidate that remains publication-ineligible until compatibility evidence exists. It still labels exact runtime switches and environment names `pending-app-owner`; the application has since recorded those in its [accepted launcher contract](../../documents/PREVIEW_RELEASE_ALIGNMENT.md#accepted-applicationlauncher-contract). The packaging owner needs to reconcile that documentation/metadata deliberately.

## Runtime-contract alignment from file reads

The current packaging values agree with the accepted application contract: launcher `Abigail Preview.exe`, Hive identifier `com.abigail.preview`, Runtime identifier `com.abigail.preview.entity-runtime`, `currentUser` installation, build selector `ABIGAIL_BUILD_PROFILE=preview`, default root `LocalAppData/AbigailPreview/profiles/ongoing`, and a persistent preview/beta marker. `stage_split_installer_resources.ps1` sets the build selector for both daemons and both shells, clears the Tauri overlay for the daemon build, and applies separate Hive/Runtime overlays. These source settings were read, not executed.

The similarly named fields belong to distinct schemas:

| Record | Meaning and ownership |
| --- | --- |
| `plugins.abigail-launch` | Static descriptive packaging metadata: `schemaVersion: 1`, `contract: "abigail-channel-profile-v1"`, packaging/application mode `channel: "preview"`, release selection `releaseChannel: "beta"`. This is not a command interpreter or a runtime process identity. |
| `profileMarker` inside that metadata | Describes the marker file: filename `profile.json`, marker schema version 1, kind `preview`, release channel `beta`, and the persistent field name `profile_id`. It does not supply the field's UUID value. |
| Actual `R/profile.json` | Application-owned persistent state with snake-case `schema_version: 1`, `kind: "preview"`, `channel: "beta"` and a generated canonical non-nil UUID in `profile_id`. The UUID persists across updates. Do not copy the packaging JSON object verbatim into this file. |
| Entity `AppConfig` | Independently versioned application schema 27. Neither packaging schema 1 nor marker schema 1 changes that version. |
| Runtime `process_identity` | Live ownership record with `channel: "beta"`, `profile_id`, `canonical_data_root`, `instance_id` and `hive_instance_id`. Its `channel` follows the marker/release channel; it is not the packaging mode string `preview`. The values are verified dynamically. |

Only policies and names belong in a packaging manifest. The application resolves the absolute root `R` on the target machine and validates it; the default relative-root descriptor is not an absolute installation-specific path. The profile UUID comes from the validated marker. Hive creates a fresh Hive/process instance UUID; each Entity daemon gets its own fresh process instance. Entity selection and loopback endpoints are supplied for that launch. Packaging must not freeze any of those values, pre-create a shared `profile_id`, choose fixed endpoint ports, or introduce a parallel `roots` layout.

The exact application interface to document is:

- Build all four binaries with `ABIGAIL_BUILD_PROFILE=preview`. This compile-time input does not replace the runtime context.
- The compiled-preview Hive launcher fills only absent `ABIGAIL_PROFILE=preview` and `ABIGAIL_DATA_DIR=R`; an explicit blank or conflicting value fails. If `ABIGAIL_DOCUMENTS_DIR` is supplied, it must equal `R/Documents`. Initial Hive launch rejects inherited `ABIGAIL_HIVE_URL`.
- Daemons receive matching `--profile preview --data-dir R`. Entity daemon also receives `--entity-id <selected-uuid>` and `--hive-url <verified-hive-url>` matching `ABIGAIL_HIVE_URL`.
- Children receive `ABIGAIL_PROFILE_ID`, `ABIGAIL_HIVE_INSTANCE_ID` and `ABIGAIL_PROCESS_INSTANCE_ID`. Runtime GUI additionally receives `ABIGAIL_ENTITY_ID`, `ABIGAIL_ENTITY_URL` and `ABIGAIL_HIVE_URL`.
- Status/descriptor identity and exact selected Entity are checked before adoption/use. Verified clients pin `x-abigail-profile-id` and `x-abigail-process-instance-id`; native verification failure has no query/environment/default-port fallback. Preview uses verified loopback endpoints outside ports `43141..43150`.
- Cleanup uses verified `POST /v1/shutdown`, acknowledgement of both `shutdown: true` and `owned_entities_reaped: true`, then exit of the owned Hive child. Uncertain cleanup does not authorize force-kill or replacement. These are application behavior requirements, not static installer switches.

### Exact gaps for the packaging owner

1. **Pending metadata and documentation.** Both overlays still contain only `runtimeInterface: { status: "pending-app-owner" }`. `docs/PREVIEW_PACKAGING.md` still says the environment names/switches await confirmation. Replace that outdated description with the interface above, while keeping installer acceptance pending. Do not reinstate the withdrawn `--channel`/`--profile-root` recipe or label the candidate ready.
2. **Validator and tests are intentionally locked to the pending schema.** `scripts/packaging_profile.ps1` requires `runtimeInterface.status == "pending-app-owner"` and `runtimeInterface.Count == 1`. It also rejects top-level `daemonArguments`, `profileId` and `roots`. `scripts/tests/test_preview_packaging.ps1` asserts that pending status and rejects a `ready` mutation. Updating an overlay alone will therefore fail validation. Any agreed descriptive runtime-interface amendment must be made consistently in both overlays, validator and tests; choose its explicit schema/status semantics with the packaging owner. The existing accepted app contract does not itself define new serialized manifest fields.
3. **Keep dynamic values out of static metadata.** The bans on fixed `profileId` and alternative `roots` remain appropriate. Rejecting a top-level manifest field named `daemonArguments` does not reject the real runtime-generated CLI arguments. They can be documented without adding that field. If a descriptive argument template is later agreed inside `runtimeInterface`, extend the strict validator/tests deliberately; never insert actual roots, UUIDs or endpoints into release artifacts.
4. **Metadata is not a runtime implementation.** Tauri plugin JSON and embedded-string checks only establish included metadata/identifiers. The application currently enforces the contract through compiled profile and validated runtime environment/arguments; the inspected native app files do not read an `abigail-launch` recipe. Do not claim the app consumes those arbitrary plugin fields. Parent-selected combined source must contain the app guards, and exact-candidate acceptance must exercise them.
5. **Reprepare after alignment.** `Get-AbigailPackagingStamp` hashes the serialized launch contract and both overlay files. The signed builder compares the prepared stamp/config before signing. Any accepted metadata change invalidates prior preparation; reprepare against the selected combined source and retain new exact hashes. Preserve the existing no-publication/compatibility-pending gate. Actual final verification/provenance, installer coexistence and all ten acceptance stages remain required.

No build-script change is needed merely to rename the already-correct `ABIGAIL_BUILD_PROFILE` selector or combine the currently separate overlay builds. The immediate coordination work is the stale pending runtime description/schema, then deliberate source integration and exact-byte validation. This pass did not edit packaging files, execute validators/tests, or establish new commit state.

## Earlier handoff claims

Task-5 `PREVIEW_RELEASE_HANDOFF_2026-10-20.md` reports the release-scaffolding branch pushed at the HEAD above and a clean worktree at `2026-10-20T11:55:04Z`. The saved `preview-push-verification.json` records that hash, unchanged main/beta refs, and no preview publication/selection.

That handoff claims offline/synthetic passes of 59/59 selector/public-metadata checks, 18/18 release policy checks, 22/22 candidate-to-beta staging checks, 9/9 existing staging checks, 6/6 download checks, and 4/4 plus 12/12 extracted workflow guards. It explicitly says those checks do not establish compatible preview application behavior or exact-installer acceptance.

Those claims belong to the earlier committed release scaffold. They do not validate the newer uncommitted packaging files. This inspection did not rerun tests or verify network remotes. At that earlier read-only checkpoint, no saved execution report for the separately reported packaging-policy and mocked-staging checks was found in the targeted root handoffs and packaging documents. The newer owner report below supplies reported results; it is a separate evidence source, not an independent rerun by this application task.

## Latest coordination: parent-relayed owner report

The parent subsequently reported that the packaging task recovered and verified its own checkout: task-5 `abigail-preview-packaging`, branch `dev/preview-packaging-0.0.76`, based on `6b246d3ab07322a899106d1198bfafba3a7b2309`. After asking its owner to commit and push only the reviewed slice, the parent confirmed commit **`de65e7e47fa35c7b742421274db5e1fac4720f8d`** on that development branch, matching local and remote hashes and a clean worktree. Main and beta remained unchanged. The owner's delivery record is task-5 `PREVIEW_PACKAGING_HANDOFF_2026-10-20.md`.

This parent-confirmed delivery supersedes the earlier uncommitted/push-pending status. This application task did not reinspect task-5 or independently recheck Git after that confirmation; the table and dirty-file observations above remain explicitly historical.

The recovered owner reported these completed checks:

| Check family | Reported result |
| --- | --- |
| Packaging policy | 42 passed |
| Mocked staging | 21 passed |
| Launcher checks | 23 passed |
| NSIS compilation checks | 32 passed |
| Release selector | 76 passed |
| Sandbox junction coverage | 1 skipped |

These are parent-relayed owner results. This application task did not rerun them or inspect new raw outputs, and they must not be added to the earlier scaffold counts as a combined total. NSIS compilation is not installer execution. The owner reported no installer run or signing; `runtimeInterface` remains pending. The runtime-schema alignment gaps above and exact-installer compatibility requirements therefore remain open.

## Inspection limits and pending work

Fresh Git worktree status/diff calls could not complete in this read-only context: attempts reported “must be run in a work tree” or “cannot come back to cwd: Permission denied.” Direct HEAD-versus-file text comparison independently confirms packaging changes, but the release checkout's cleanliness was not freshly verified. This was a read-only tooling failure, not an approval denial or an uncertain write.

Packaging commit/push is now parent-confirmed at `de65e7e47fa35c7b742421274db5e1fac4720f8d`; it does not establish source integration or runtime/installer acceptance. Exact signed-installer provenance, generated identity inspection, coexistence, application compatibility and preview publication remain pending. The packaging owner retains responsibility for its remaining work; no integration or takeover is implied.
