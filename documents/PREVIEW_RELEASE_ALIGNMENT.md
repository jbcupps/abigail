# Application and preview-release alignment

2026-10-20. Compared application development commit `d883fe001c31b1999d76a7bf5a7defd3818e2744` with release scaffolding `6b246d3ab07322a899106d1198bfafba3a7b2309`, specifically its `docs/PREVIEW_CHANNEL.md` and the release owner's application handoff. The branches remain separate. This document records coordination and component evidence, not installed-preview acceptance.

The parent subsequently confirmed the separate packaging slice committed and pushed at `de65e7e47fa35c7b742421274db5e1fac4720f8d`. The [packaging alignment handoff](../output/playwright/preview-packaging-readonly-handoff.md) distinguishes that owner-reported status from the earlier read-only inspection and lists the coordinated runtime-metadata changes still required. The application slice's [final component evidence](../output/playwright/preview-profile-isolation-validation.md) does not transfer acceptance to a combined build or installer.

## Agreement and additions

The contracts agree: preview must coexist with stable, keep one continuing Entity across updates, use independent disposable fixtures for creation, and preserve exact source/installer identity. The application term **preview profile** maps to a selected **beta release**, not a third release channel. The release descriptor remains unavailable until all exact-installer checks pass.

The release handoff adds explicit requirements to the application proposal:

- Verify **channel, profile/root, and live process-instance identity** before adopting, closing, or restarting a daemon. Preserve handle-scoped termination and never kill a stale descriptor PID. Validate packaged child paths and reject conflicting inherited executable/endpoint overrides.
- Reject unsupported newer profile/config schemas before initialization writes. A guarded config loader is one component; it is not whole-profile startup preflight.
- Keep a fresh-install creation fixture separate from the continuing-profile upgrade fixture, in addition to testing two Entity scopes within one synthetic profile.
- Use the ten stage IDs below unchanged when producing exact-source/installer evidence.

The application proposal also makes credential namespace, ambient provider-account reuse, WebView storage, saved creation-journal endpoints, and restored absolute-path handling explicit. Those details complement the release contract; a custom `ABIGAIL_DATA_DIR` alone does not satisfy it.

## Accepted application/launcher contract

The parent accepted this implementation target on 2026-10-20 after assigning the packaging product and identifiers. The current development source implements the application profile/preflight, path, credential namespace and connection guards described in [PREVIEW_PROFILE_CONTRACT.md](PREVIEW_PROFILE_CONTRACT.md). Final component validation is recorded separately. This is not installed-preview acceptance or a recipe for using real profiles.

| Parameter | Accepted value or invariant |
| --- | --- |
| Release channel | Existing beta tags, `vX.Y.Z-beta.N`; no new release channel |
| Compiled application profile | Build-time `ABIGAIL_BUILD_PROFILE=preview` embedded in all four binaries; a compiled preview must not become stable when runtime variables are absent |
| Required launcher mode/root | `ABIGAIL_PROFILE=preview` and an absolute persistent `ABIGAIL_DATA_DIR=R`; the compiled-preview Hive launcher fills only absent values with preview/default root. Explicit blank/conflicting values fail. Child daemons require explicit matching context. |
| Child daemon arguments | `--profile preview --data-dir R`; these must agree with the inherited validated context |
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

The marker's schema is independent of Entity config schema 27. Persist the profile UUID across versions. Initial marker creation is allowed only in a new empty validated root; a populated unmarked root requires an explicit import decision.

After marker verification, the app supplies `ABIGAIL_PROFILE_ID=<uuid>` to its children. For a new Hive launch it generates `ABIGAIL_HIVE_INSTANCE_ID=<fresh-uuid>` and sets `ABIGAIL_PROCESS_INSTANCE_ID` to the same UUID. Each Entity child receives the expected Hive instance and its own fresh process instance, with the explicitly selected Hive URL. Status/descriptor `process_identity` contains `channel`, `profile_id`, `canonical_data_root`, `instance_id`, and `hive_instance_id`. Profile/root/instance values must be verified before adopting an existing process or sending close/recovery requests. These non-secret identifiers identify ownership; they are not provider credentials. Never trust inherited `ABIGAIL_HIVE_URL` or executable overrides as ownership proof. Reject a conflicting inherited value rather than silently rebinding.

Verified preview HTTP clients pin `x-abigail-profile-id` and `x-abigail-process-instance-id`; preview daemons require these except for GET `/health`, GET `/v1/status`, and OPTIONS. Current stable middleware rejects preview ownership headers. These are routing guards against accidental cross-profile use, not authentication credentials or binary provenance. Preview endpoints use verified loopback URLs outside stable's reserved ports `43141..43150`. The initial Hive launcher refuses inherited `ABIGAIL_HIVE_URL`; child processes receive their verified endpoint explicitly.

Native connection resolution fails closed when handshake/IPC verification fails, with no browser/query/environment/default-port fallback. A stored creation-journal endpoint that no longer matches the pinned preview daemon is rejected explicitly; the exact creation key is preserved. Automatic same-profile endpoint rebinding is not implemented.

The preview cleanup protocol is verified `POST /v1/shutdown`, acknowledging `shutdown: true` and `owned_entities_reaped: true`, followed by exit of the owned Hive child. An uncertain drain must not trigger force-kill/replacement. Close requests identify the exact Entity process instance. These source guards still require actual candidate lifecycle acceptance.

The assigned packaging worker owns Tauri packaging configs/overlays, `scripts/stage_split_installer_resources.ps1`, `scripts/build_signed_installer.ps1`, and release tests. This app worktree will not edit those files. Application engineering owns Rust config/profile/path/process code and app continuity tests. Coordinate the two Tauri compilation configurations: current staging uses one shared `TAURI_CONFIG` during a multi-package build, so a shared override cannot safely assign different Hive/Runtime identifiers. Preview overlays should preserve stable defaults.

The parent selects the integration source. Current app and release branches are main-based; selected beta Genesis/static-runtime/installer-lock behavior still needs deliberate reconciliation with main signing/provenance safeguards. No merge or source selection is implied by this comparison.

## Implementation sequence and remaining work

1. **Config refusal — implemented:** validate the serialized version before deserializing/migrating, guard saves against a newer on-disk target, and stop active callers from substituting defaults or consuming a backup after refusal. Schema 27 remains current; earlier component results are linked below.
2. **Startup/profile context — implemented in source:** validate compiled mode, root, marker schema, active configs and registry before startup consumers initialize. Use one immutable context for the paths in [PREVIEW_PROFILE_CONTRACT.md](PREVIEW_PROFILE_CONTRACT.md). Stable defaults remain unchanged.
3. **Credential and connection dependencies — implemented in source:** scope credential selection/cache/encryption labels, with fake-adapter test seams. Bind channel/profile/root/live instance through daemon discovery, child launch, saved creation recovery and close/restart. Reject native fallback on failure. Component tests do not validate real OS credentials or installed lifecycle.
4. **Full cold snapshot/restore — next slice, unimplemented:** quiesce owners; inventory all persistent files and required credential availability; restore into a new fixture; rebase approved paths, invalidate descriptors, and verify the same UUID, history and settings. Existing per-Entity restore generates a new UUID and does not satisfy this requirement.
5. **Exact candidate acceptance — open:** coordinate distinct compiled/installer identities and run the ten stages against the final signed installer bytes. Preserve the accepted artifact; component tests cannot transfer acceptance to a rebuilt installer.

## Release-stage evidence mapping

| Exact stage ID | Application contribution / remaining evidence |
| --- | --- |
| `separate-installer-identity` | Assigned identities above; packaging is owned separately. Inspect the final compiled/generated installer identity and provenance. |
| `stable-preview-coexistence` | Requires isolated synthetic Windows install/upgrade/uninstall in both launch orders; no personal-profile run. |
| `profile-path-isolation` | Shared context/preflight, scoped paths and synthetic rejection/sentinel tests are implemented. Exact installed candidate evidence remains open. |
| `daemon-ownership-isolation` | Profile/root/live-instance handshakes, pinned transport and owned-child lifecycle guards are implemented. Actual candidate lifecycle/port-race acceptance remains open. |
| `fresh-creation-fixture` | Keyed creation component tests exist; use a separate fresh installed fixture for the existing daemon contract. |
| `ongoing-entity-upgrade` | Synthetic identity/profile preservation tests exist. First preview needs an explicit seeded baseline source/hash, not an invented prior accepted release. |
| `ongoing-entity-reopen` | UI restoration and four-phase scoped-store subprocess evidence exist; actual Hive/Runtime restart on the final candidate remains. |
| `full-cold-backup-restore` | No full snapshot/restore implementation or candidate acceptance is claimed. |
| `newer-schema-refusal` | Config/caller refusal and active-profile startup preflight are implemented. Exact installed rejection with byte-preserved fixtures remains open. |
| `return-to-stable` | Stable stays on its own unchanged profile; preview rollback requires a separately preserved compatible snapshot. |

Earlier config refusal results are recorded in [schema-refusal validation](../output/playwright/preview-schema-validation.md). The isolation slice's [final component evidence](../output/playwright/preview-profile-isolation-validation.md) records 167 selected Rust tests and 373 frontend tests; these stage rows do not mark any signed-installer stage accepted. No real credential operation, normal daemon/installer launch, release publication, main/beta merge, or real-data migration is part of this bounded change.
