# Maintained/disposable Entity validation

Date: 2026-10-20. Development branch: `dev/first-signal-integration-0.0.76`.
This follow-up extends the First Signal integration at `30cbbca`. All content and stores used for validation are synthetic. Actual user Entities, Documents, credentials, and provider accounts were not inspected or changed.

## Changes and confirmed results

| Check | Command | Result |
| --- | --- | --- |
| Hive UI | `npm --prefix hive-app/src-ui test` | 269/269 pass |
| Runtime UI | `npm --prefix entity-runtime-app/src-ui test` | 74/74 pass |
| Both UI builds | `npm --prefix <app>/src-ui run build` | TypeScript and Vite pass |
| Identity creation compatibility | `cargo test --locked --offline -p abigail-identity -j 1 creation::tests` | 18/18 pass |
| Profile router/storage | `cargo test --locked --offline -j 1 -p hive-daemon --test presence_routes -- --test-threads=1` | 12/12 pass |
| Scoped store restart | `cargo test --locked --offline -p abigail-persistence client::tests -- --test-threads=1` | 3/3 pass, including four fresh subprocess phases |

Commands above run from the repository root. The UI total is **343**, replacing the earlier 332; it is not added to that count. Rust and UI runs use workspace target/temp directories. The previous integration's six creation-handler and three wire-contract results still apply to unchanged implementation, but were not rerun for this follow-up.

Actual captured command output is available in:

- [Hive tests](preview-hive-tests.txt)
- [Hive build](preview-hive-build.txt)
- [Identity tests](preview-identity-tests.txt)
- [Scoped persistence test](integration-continuity-persistence.txt)

These copies redact machine paths, remove terminal control sequences, and trim trailing whitespace. Runtime and profile test/build results above were observed in tool output; no reconstructed raw transcript is presented for them.

The selected-Entity regression first demonstrated that a mismatched runtime could display another Entity's transcript. The corrected UI checks the restored Entity ID before showing messages or enabling Send. A failed identity lookup has a bounded error and Retry path. Hive helper opening requires its authoritative nonempty ID; an already-open helper retains that ID across polling and port changes. Independent review confirmed the final helper guard.

Native Runtime can only pin its initial status ID because its native launch contract supplies an address, not an independently selected Entity ID. Status/history requests precede the display gate. This is neither continuous identity verification nor a complete preview-profile endpoint boundary.

The identity tests preserve a populated legacy registry, retained Entity UUID/config/docs/presentation bytes, keyed recovery, and disposable create/replay/delete/reload behavior. They use the inert isolated manager, not normal credential/bootstrap paths. The profile tests exercise actual Axum/storage code with a synthetic registry, including no-sidecar defaults, reopen, rejected writes, and preserved ongoing files. Synthetic removal is not actual Entity DELETE acceptance.

The first persistence run completed with a fixture failure: its Hive-scope query addressed a table never created in that empty scope. The correction defines only that empty test table; it does not change production schema or suppress errors. The corrected run passes exact content assertions for two Entity databases with identical record/session IDs, empty Hive results, disposable-record removal, and preserved ongoing content after another process reopen. The subprocess result is storage evidence, not a daemon or installer restart result.

## Browser evidence

The earlier integrated browser evidence remains valid for its recorded revision and synthetic transport. This follow-up did not run another browser session:

- [Integrated home](14-integrated-home.jpg)
- [Recovered reveal](17-recovered-reveal.jpg)
- [Restored conversation](18-restored-conversation.jpg)

The task-owned preview and fixture servers were stopped. These images are not screenshots of a running persistent preview profile. Earlier 01-13 images are historical prototype evidence.

## Remaining boundaries

The [preview profile contract](../../documents/PREVIEW_PROFILE_CONTRACT.md) is a source-based proposal, **not implemented configuration**. Setting the proposed variables on current code does not isolate preview data. Current native tool roots, archive/backup discovery, logs, WebView state, credential namespace, and fallback endpoints can reach normal-profile locations. Application and packaging owners must agree the contract before broad changes.

Normal-daemon acceptance remains held before startup because the existing hybrid provider reads the OS keyring before raw test-key fallback. The exact call chain and test dependency-injection proposal are in [STARTUP_TEST_ISOLATION_PLAN.md](../../documents/STARTUP_TEST_ISOLATION_PLAN.md). No daemon or credential operation was attempted for this follow-up.

Narrow-screen inspection remains uncertain: the earlier `Emulation.setDeviceMetricsOverride` request for the local test tab returned after the platform had reported approval pending, and no independent approval resolution was available. It was not repeated. No mobile pass is claimed.

An actual signed-candidate update and whole-root backup/restore have not been tested. Production use needs a persistent validated root, explicit profile identity, one Hive owner, and verified restoration. Full erasure of deleted Entity history, arbitrary legacy SQLite migration, turn-scoped server cancellation, and concurrent profile-write conflict handling remain outside these completed checks. See [data compatibility](../../documents/PREVIEW_DATA_COMPATIBILITY.md).

Release workflows, release verification scripts, Cargo.lock, native path/credential selection, and default production identifiers are unchanged in this follow-up. No main/beta merge, release tag, installer, or production deployment was performed.
