# Application and preview-release alignment

2026-10-20. The selected isolated combined development branch is `dev/preview-combined-0.0.76`. It integrates these published sources:

| Input | Exact source |
| --- | --- |
| Application, `dev/first-signal-integration-0.0.76` | `ca6a7527da8787be6b5315e13e4fc228287ce15d` |
| Cold-recovery descendant on the same application branch | `4558fe68d9812998eb53a94ba157970af552f923` |
| Packaging, `dev/preview-packaging-0.0.76` | `de65e7e47fa35c7b742421274db5e1fac4720f8d` |
| Release scaffold included by packaging | `6b246d3ab07322a899106d1198bfafba3a7b2309` |
| Shared stable-main base | `17b636b9e5f10b812deb287ad2c6637188991ec2` |

Remote heads were checked before integration; no combined remote branch existed. The prior worktrees remain untouched. The [earlier packaging handoff](../output/playwright/preview-packaging-readonly-handoff.md) is historical coordination evidence. The combined source aligns both preview overlays' descriptive `runtimeInterface`, validator, tests and [packaging documentation](../docs/PREVIEW_PACKAGING.md) with the implemented application contract below. Overlay, contract, source and executable hashes must be prepared again after alignment.

The application's [167 selected Rust and 373 frontend passes](../output/playwright/preview-profile-isolation-validation.md), the published recovery slice's [104 distinct Rust tests and seven CLI checks](../output/playwright/cold-profile-validation.md), and prior packaging checks are separate historical component evidence. The three ignored recovery child fixtures are not extra passes. They do not establish combined lifecycle or installed-preview acceptance. Recovery source publication was verified at the full SHA above; publication-pending wording in its archived component record describes that earlier checkpoint. This dev integration does not sign, execute an installer, publish a release, advance the unavailable preview selector, access real credentials or profiles, grant new access, or change main/beta. Stable 0.0.76 remains available.

See [combined development validation](../docs/PREVIEW_COMBINED_VALIDATION.md) for this candidate's fresh checks and native build/preparation evidence.

## Agreement and additions

The contracts agree: preview must coexist with stable, keep one continuing Entity across updates, use independent disposable fixtures for creation, and preserve exact source/installer identity. The application term **preview profile** maps to a selected **beta release**, not a third release channel. The release descriptor remains unavailable until all exact-installer checks pass.

The release handoff adds explicit requirements to the application proposal:

- Verify **channel, profile/root, and live process-instance identity** before adopting, closing, or restarting a daemon. Preserve handle-scoped termination and never kill a stale descriptor PID. Validate packaged child paths and reject conflicting inherited executable/endpoint overrides.
- Reject unsupported newer profile/config schemas before initialization writes. A guarded config loader is one component; it is not whole-profile startup preflight.
- Keep a fresh-install creation fixture separate from the continuing-profile upgrade fixture, in addition to testing two Entity scopes within one synthetic profile.
- Use the ten stage IDs below unchanged when producing exact-source/installer evidence.

The application proposal also makes credential namespace, ambient provider-account reuse, WebView storage, saved creation-journal endpoints, and restored absolute-path handling explicit. Those details complement the release contract; a custom `ABIGAIL_DATA_DIR` alone does not satisfy it.

## Accepted application/launcher contract

This implementation target was accepted on 2026-10-20 after assignment of the packaging product and identifiers. The current development source implements the application profile/preflight, path, credential namespace and connection guards described in [PREVIEW_PROFILE_CONTRACT.md](PREVIEW_PROFILE_CONTRACT.md). Previous isolation evidence and final cold-recovery component validation are recorded separately. This is not installed-preview acceptance or a recipe for using real profiles.

| Parameter | Accepted value or invariant |
| --- | --- |
| Release channel | Existing beta tags, `vX.Y.Z-beta.N`; no new release channel |
| Compiled application profile | Build-time `ABIGAIL_BUILD_PROFILE=preview` embedded in all four binaries; a compiled preview must not become stable when runtime variables are absent |
| Required launcher mode/root | `ABIGAIL_PROFILE=preview` and an absolute persistent `ABIGAIL_DATA_DIR=R`; the compiled-preview Hive launcher fills only absent values with preview/default root. Explicit blank/conflicting values fail. Child daemons require explicit matching context. |
| Child daemon arguments | `--profile preview --data-dir R`; these must agree with the inherited validated context |
| Daemon ports | Hive receives `--port <chosen-port>` outside stable's reserved `43141..43150`; Entity receives `--port 0`. Endpoints are selected and verified at launch. |
| Entity daemon selection | Explicit `--entity-id <uuid>` and `--hive-url <verified-url>`; the Hive URL must match `ABIGAIL_HIVE_URL` |
| Runtime GUI selection | `ABIGAIL_ENTITY_ID`, `ABIGAIL_ENTITY_URL` and `ABIGAIL_HIVE_URL`, plus the same validated profile and instance context |
| Profile marker | `R/profile.json`: `schema_version: 1`, `kind: "preview"`, `channel: "beta"`, persistent non-secret `profile_id` UUID |
| Hive identifier (assigned) | `com.abigail.preview` |
| Runtime identifier (assigned) | `com.abigail.preview.entity-runtime` |
| User-facing product/shortcut (assigned) | `Abigail Preview` |
| Installation identity | Distinct preview install/uninstall registration and directory; do not rely on `/D` |
| Build identity | Full beta tag plus source SHA; numeric version alone cannot distinguish beta iterations |
| Executable selection | Explicit packaged child paths; no implicit reuse of stable overrides |

The version-independent default ongoing root is `<KnownFolder LocalAppData>/AbigailPreview/profiles/ongoing`, or an explicit user-selected alternative validated against production overlap. The compiled-preview Hive launcher resolves that absolute preview location only when `ABIGAIL_DATA_DIR` is absent; daemons never substitute the normal KnownFolder data path. No actual directory at that default location was read or created for this application validation. If supplied, `ABIGAIL_DOCUMENTS_DIR` must equal `R/Documents`.

Fresh-creation fixture roots are separate unique directories beneath the test workspace, such as `<workspace>/target/preview-fixtures/<run-id>/creation`. Continuing-profile upgrade/reopen tests use a distinct `<run-id>/ongoing` fixture retained across candidate updates. Within-profile disposable-Entity tests remain useful, but reset/cleanup targets must never be the continuing root. Synthetic harnesses must require their explicit fixture root, not use the persistent launcher location.

The marker's schema is independent of Entity config schema 27. Persist the canonical non-nil profile UUID across versions. Initial marker creation is allowed only in a new empty validated root; children cannot initialize a missing marker. A populated unmarked root requires an explicit import decision.

After marker verification, the app supplies `ABIGAIL_PROFILE_ID=<uuid>` to its children. For a new Hive launch it generates `ABIGAIL_HIVE_INSTANCE_ID=<fresh-uuid>` and sets `ABIGAIL_PROCESS_INSTANCE_ID` to the same UUID. Each Entity child receives the expected Hive instance and its own fresh distinct process instance, with the explicitly selected Hive URL. Status/descriptor `process_identity` contains `channel`, `profile_id`, `canonical_data_root`, `instance_id`, and `hive_instance_id`; preview channel is `beta`. Verify every field and the selected Entity before adoption or use. These non-secret identifiers identify ownership; they are not provider credentials. Never trust inherited `ABIGAIL_HIVE_URL` or executable overrides as ownership proof. Reject blank, relative or conflicting inherited `ABIGAIL_ENTITY_DAEMON_PATH`, `ABIGAIL_ENTITY_RUNTIME_APP_PATH` and `ABIGAIL_INTERNAL_BIN_DIR`; use explicit canonical packaged child paths.

Verified preview HTTP clients pin `x-abigail-profile-id` and `x-abigail-process-instance-id`; preview daemons require these except for GET `/health`, GET `/v1/status`, and OPTIONS. Current stable middleware rejects preview ownership headers. These are routing guards against accidental cross-profile use, not authentication credentials or binary provenance. Preview endpoints use verified loopback URLs outside stable's reserved ports `43141..43150`. The initial Hive launcher refuses inherited `ABIGAIL_HIVE_URL`; child processes receive their verified endpoint explicitly.

Native connection resolution fails closed when handshake/IPC verification fails, with no browser/query/environment/default-port fallback and no followed redirects. A stored creation-journal endpoint that no longer matches the pinned preview daemon is rejected explicitly; the exact creation key is preserved. Automatic same-profile endpoint rebinding is not implemented.

Entity close uses `POST /v1/entities/:id/close` with `{"expected_entity_instance_id":"<exact Entity daemon process UUID>"}` and validates the API envelope. The preview cleanup protocol is verified `POST /v1/shutdown`, acknowledging `{ok:true,data:{shutdown:true,owned_entities_reaped:true}}`, followed by exit of the owned Hive child. An uncertain drain must not trigger force-kill/replacement; never kill a stale descriptor PID. These source guards still require actual candidate lifecycle acceptance.

All four preview processes retain a shared profile-use lease until exit. Cold operations require exclusive profile-use and Hive-owner leases, and normal startup rejects reserved cold-staging paths and their descendants. The lifecycle metadata describes these implemented invariants without a concrete root or process ID. Cooperative locks do not detect unknown profile clones or every older/foreign writer, and they do not prevent separately restored roots from being opened later. The offline `abigail-profile` recovery utility remains separate from the four packaged application binaries.

The packaging source builds daemons without a Tauri overlay, then Hive and Runtime separately using their own `TAURI_CONFIG` overlays. All four binaries must embed `ABIGAIL_BUILD_PROFILE=preview`. Stable base configurations and default build behavior remain preserved. The overlay metadata describes this interface; application code does not execute arbitrary plugin JSON as a launch recipe. Dynamic roots, profile/process UUIDs, selected Entity IDs, ports and URLs must never become static release metadata. Fixed `profileId`, alternative `roots` and a static `daemonArguments` recipe remain prohibited.

The parent selected the main-based published app and packaging inputs, then authorized adding the app's published cold-recovery descendant. Beta `ea92effd31f554f74bb5e109deaa42876ec4f06f` Genesis/static-runtime/installer-lock behavior remains separate for explicit later comparison with main signing/provenance safeguards. No blind beta merge, main/beta push or release promotion is part of this task.

## Implementation sequence and remaining work

1. **Config refusal — implemented:** validate the serialized version before deserializing/migrating, guard saves against a newer on-disk target, and stop active callers from substituting defaults or consuming a backup after refusal. Schema 27 remains current; earlier component results are linked below.
2. **Startup/profile context — implemented in source:** validate compiled mode, root, marker schema, active configs and registry before startup consumers initialize. Use one immutable context for the paths in [PREVIEW_PROFILE_CONTRACT.md](PREVIEW_PROFILE_CONTRACT.md). Stable defaults remain unchanged.
3. **Credential and connection dependencies — implemented in source:** scope credential selection/cache/encryption labels, with fake-adapter test seams. Bind channel/profile/root/live instance through daemon discovery, child launch, saved creation recovery and close/restart. Reject native fallback on failure. Component tests do not validate real OS credentials or installed lifecycle.
4. **Full cold snapshot/restore - included from published application source:** the [Windows same-user/device CLI](PREVIEW_COLD_RECOVERY.md) requires exclusive cold/Hive leases, inventories the whole retained root and shared store, probes existing credentials, rebases only declared paths and publishes a fresh destination without copied runtime descriptors. Prior core/store and CLI evidence is tracked [separately](../output/playwright/cold-profile-validation.md); fresh combined checks belong to [combined validation](../docs/PREVIEW_COMBINED_VALIDATION.md). Existing per-Entity restore generates a new UUID and remains unchanged. Real OS credentials, normal GUI reopening and installed recovery remain open, and the offline utility is not bundled by this integration.
5. **Exact candidate acceptance - open:** the combined source supplies distinct compiled/installer identities; actual lifecycle and Windows install/upgrade/uninstall/restore acceptance remain pending. Run the ten stages against the final signed installer bytes in a separately authorized acceptance task. Preserve the accepted artifact; component tests cannot transfer acceptance to a rebuilt installer.

## Release-stage evidence mapping

| Exact stage ID | Application contribution / remaining evidence |
| --- | --- |
| `separate-installer-identity` | Combined source supplies the assigned identities above. Inspect the final compiled/generated installer identity and provenance. |
| `stable-preview-coexistence` | Requires isolated synthetic Windows install/upgrade/uninstall in both launch orders; no personal-profile run. |
| `profile-path-isolation` | Shared context/preflight, scoped paths and synthetic rejection/sentinel tests are implemented. Exact installed candidate evidence remains open. |
| `daemon-ownership-isolation` | Profile/root/live-instance handshakes, pinned transport and owned-child lifecycle guards are implemented. Actual candidate lifecycle/port-race acceptance remains open. |
| `fresh-creation-fixture` | Keyed creation component tests exist; use a separate fresh installed fixture for the existing daemon contract. |
| `ongoing-entity-upgrade` | Synthetic identity/profile preservation tests exist. First preview needs an explicit seeded baseline source/hash, not an invented prior accepted release. |
| `ongoing-entity-reopen` | UI restoration and four-phase scoped-store subprocess evidence exist; actual Hive/Runtime restart on the final candidate remains. |
| `full-cold-backup-restore` | Published recovery source includes bounded Windows snapshot/verify/restore. Its prior component record passed 101 core plus three actual SurrealKV fresh-process tests and seven separate CLI smoke checks; combined reruns are recorded separately. Actual recovery using the exact installed candidate and existing OS credentials remains open. |
| `newer-schema-refusal` | Config/caller refusal and active-profile startup preflight are implemented. Exact installed rejection with byte-preserved fixtures remains open. |
| `return-to-stable` | Stable stays on its own unchanged profile; preview rollback requires a separately preserved compatible snapshot. |

Earlier config refusal results are recorded in [schema-refusal validation](../output/playwright/preview-schema-validation.md). The previous isolation slice's [final component evidence](../output/playwright/preview-profile-isolation-validation.md) records 167 selected Rust tests and 373 frontend tests. Those historical counts are not new cold-recovery results; the [final cold component record](../output/playwright/cold-profile-validation.md) separately records 104 Rust passes and seven CLI checks. These stage rows do not mark any signed-installer stage accepted. No real credential operation, normal daemon/installer launch, release publication, main/beta merge, or real-data migration is part of this bounded change.
