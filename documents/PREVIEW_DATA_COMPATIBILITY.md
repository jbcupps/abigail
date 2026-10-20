# First Signal preview: maintained and disposable Entities

Application/data compatibility review, 2026-10-20. The intended preview workflow is one ongoing Entity whose identity and history survive application updates, alongside disposable Entities used for creation and maintenance checks. This review uses only synthetic data. It does not change release workflows, channels, installer routing, or actual user Entities.

## Data compatibility boundary

Compared with signed stable source `e3ad511`, this integration does not change the persistence engine/schema, `AppConfig` migration code/schema version 27, native app data-root selection, or Tauri application identifiers. Identity directories remain `identities/<UUID>`, and the existing Hive owns the shared `memory.db`. Presentation is a separate `presence.json` sidecar; an existing Entity without one gets non-writing defaults. Invalid saved presentation is reported, not silently reset.

The new global `creation_attempts` field has a deserialization default. A populated prior registry can load without it; future keyed creation adds receipts while retaining the prior registered IDs, directories, and settings. An existing born Entity is opened without re-running the new-Entity quickstart. Imported visual briefs remain presentation metadata, not a rewrite of existing identity documents or permissions.

This is forward compatibility with the inspected stable data format. It is not proof of an installed-version upgrade or arbitrary downgrade compatibility. An older binary can deserialize unknown registry fields and later omit them when saving, so rolling back across the creation-receipt addition is not a supported way to preserve pending creation recovery. A pre-update data snapshot and restoration procedure require separate release acceptance.

## Evidence and limits

| Area | Current evidence | Limit |
| --- | --- | --- |
| Existing identity and new receipts | 18 actual isolated IdentityManager tests pass, including a populated legacy registry, same-name disposable creation/replay/deletion, preserved ongoing UUID/files, and recovery of a partially initialized disposable Entity after manager reload. | Synthetic signing key/registry constructor; not normal credential bootstrap, installed upgrade, or a cryptographic recovery acceptance test. |
| Presentation | 12 actual Axum profile-router/storage tests pass. Existing no-sidecar Entities receive defaults without mutating config/identity files; save/reopen and disposable writes/removal leave the ongoing profile and files unchanged. | Registry removal is a synthetic test operation, not a real DELETE endpoint acceptance result. |
| Conversation and process restart | Three persistence client tests pass, including four fresh subprocess phases with two Entity scopes sharing session/turn IDs, an empty Hive scope, and removal of one disposable turn. Exact ongoing content survives the final reopen. | Test subprocesses are not Hive/Entity daemon startup or installer processes. Record deletion is not full Entity/history erasure. |
| Selected Entity in the UI | Runtime rejects a restored identity different from the selected Entity before displaying the transcript or enabling Send. Helper opening requires an authoritative Hive Entity ID, retained across polling/port changes. All 269 Hive and 74 Runtime UI tests pass; both TypeScript/Vite builds pass. | Native Runtime pins its initial status ID because its launch contract has no independently supplied selected ID. Validation occurs after read-only status/history requests and is not continuous identity verification. |

## Operational constraints for the first usable preview

- Keep the ongoing Entity's UUID and Hive data root unchanged. Use one Hive owner for that root; do not run different app versions concurrently against it.
- Use separate Entity IDs for disposable work, not just another session in the ongoing Entity. Recall can span sessions within one Entity, and normal reopen selects its latest session. A whole Hive data-root snapshot is necessary: identity directories alone omit the shared `memory.db` SurrealKV directory. It is currently insufficient for complete recovery because Documents/archive output, WebView storage, and OS credential entries can be outside it. Recovery needs an explicit external-artifact inventory and verified credential availability. No actual user data was read or copied for this review.
- Give disposable Entities distinct names, including after Windows case folding and filename sanitization. Auxiliary `Documents/Abigail/<sanitized name>` paths are name-based (`crates/abigail-identity/src/lib.rs:686-742`), and recovery bundles use a fixed filename there. Equal or equivalently sanitized names can share that folder even though identity/profile/conversation storage is UUID-scoped. This review does not move existing Documents or claim complete filesystem isolation.
- The shell adopts any healthy recorded daemon (`hive-app/src/lib.rs:584-607`). A frontend-only relaunch therefore does not prove that updated daemon binaries are running. Candidate acceptance must verify the intended binary revisions after stopping/restarting the app through its supported lifecycle.
- A changed browser origin can lose browser-local drafts/session hints. The authoritative Entity registry, presentation, and committed conversation history are server-side; browser storage is not a replacement for those files. A cleared creation key cannot be reconstructed safely from an Entity name.
- Existing Entity deletion removes its registry entry and identity directory; it does not purge that Entity's records from the shared database. The disposable tests do not claim complete history erasure. Also avoid the combined `ABIGAIL_CI_MODE`/`ABIGAIL_DAEMON_INTEGRATION` path for durability acceptance: it deliberately substitutes ephemeral memory storage.
- `migrate_legacy_layout` exists in `crates/abigail-persistence/src/migration.rs`, but no active caller was found in the inspected source. Do not claim automatic migration of arbitrary old SQLite layouts. This integration does not change that legacy path.
- Stopping a profile PUT can leave an uncertain accepted server write. Its current contract has no revision precondition; avoid simultaneous profile editing from multiple clients during this preview.
- The inherited server cancellation slot is not turn-scoped (`crates/entity-daemon/src/routes.rs:427,517`). A delayed Stop for an earlier turn or that turn's cleanup can affect a newer turn after rapid resend. The UI tests do not validate server ownership of cancellation; a separate bounded fix needs deterministic overlapping-turn handler tests.

## Minimum remaining acceptance before recommending a signed preview update

**Profile isolation is now a blocking prerequisite.** A custom data root does not currently constrain all native tool, archive, backup, log, WebView, and credential paths. The source-based [preview profile contract](PREVIEW_PROFILE_CONTRACT.md) specifies the proposed minimum packaging/application boundary before broad changes. No isolated-production-preview claim is made for the current build.

1. Implement and test the agreed application profile boundary before claiming that the preview is isolated. The selected-Entity regression is fixed and independently reviewed; the recorded synthetic tests are narrower than installed acceptance.
2. On a disposable maintained-profile fixture, verify the actual signed candidate update/restart preserves the same UUID, identity/certificate files, presentation, and exact committed transcript, while creating/removing a distinct disposable Entity leaves the maintained one intact. Verify the expected native/daemon revisions and unchanged data root. The release-engineering owner controls this candidate and installer check.
3. Resolve how normal daemon startup is exercised without touching the operator's existing credentials. The exact keyring access and a narrower test-only injection proposal are in [STARTUP_TEST_ISOLATION_PLAN.md](STARTUP_TEST_ISOLATION_PLAN.md). Current in-process and persistence tests do not close that production bootstrap gap.

The unresolved viewport approval limits narrow-screen evidence; no current mobile-layout pass is claimed. It does not establish a failure of the desktop layout. Actual generated video/audio, beta Genesis reconciliation, complete skill lifecycle work, and durable cross-process bus replacement are not prerequisites for evaluating this existing procedural desktop interaction slice.

Exact commands, results, screenshots, and remaining held actions are recorded in [preview validation](../output/playwright/preview-continuity-validation.md).
