# Application and preview-release alignment

2026-10-20. Compared application development commit `d883fe001c31b1999d76a7bf5a7defd3818e2744` with release scaffolding `6b246d3ab07322a899106d1198bfafba3a7b2309`, specifically its `docs/PREVIEW_CHANNEL.md` and the release owner's application handoff. The branches remain separate. This document records coordination and component evidence, not installed-preview acceptance.

## Agreement and additions

The contracts agree: preview must coexist with stable, keep one continuing Entity across updates, use independent disposable fixtures for creation, and preserve exact source/installer identity. The application term **preview profile** maps to a selected **beta release**, not a third release channel. The release descriptor remains unavailable until all exact-installer checks pass.

The release handoff adds explicit requirements to the application proposal:

- Verify **channel, profile/root, and live process-instance identity** before adopting, closing, or restarting a daemon. Preserve handle-scoped termination and never kill a stale descriptor PID. Validate packaged child paths and reject conflicting inherited executable/endpoint overrides.
- Reject unsupported newer profile/config schemas before initialization writes. A guarded config loader is one component; it is not whole-profile startup preflight.
- Keep a fresh-install creation fixture separate from the continuing-profile upgrade fixture, in addition to testing two Entity scopes within one synthetic profile.
- Use the ten stage IDs below unchanged when producing exact-source/installer evidence.

The application proposal also makes credential namespace, ambient provider-account reuse, WebView storage, saved creation-journal endpoints, and restored absolute-path handling explicit. Those details complement the release contract; a custom `ABIGAIL_DATA_DIR` alone does not satisfy it.

## Concrete configuration proposal to freeze before packaging edits

The parent assigned the packaging product and identifiers below on 2026-10-20. The launcher/profile protocol remains a proposal, **not supported switches or a launch recipe**:

| Parameter | Proposed value or invariant |
| --- | --- |
| Release channel | Existing beta tags, `vX.Y.Z-beta.N`; no new release channel |
| Compiled application profile | Build-time `ABIGAIL_BUILD_PROFILE=preview` embedded in all four binaries; a compiled preview must not become stable when runtime variables are absent |
| Required launcher mode/root | `ABIGAIL_PROFILE=preview` and an absolute persistent `ABIGAIL_DATA_DIR=R`; unset, blank or conflicting values fail |
| Child daemon arguments | `--profile preview --data-dir R`; these must agree with the inherited validated context |
| Profile marker | `R/profile.json`: `schema_version: 1`, `kind: "preview"`, `channel: "beta"`, persistent non-secret `profile_id` UUID |
| Hive identifier (assigned) | `com.abigail.preview` |
| Runtime identifier (assigned) | `com.abigail.preview.entity-runtime` |
| User-facing product/shortcut (assigned) | `Abigail Preview` |
| Installation identity | Distinct preview install/uninstall registration and directory; do not rely on `/D` |
| Build identity | Full beta tag plus source SHA; numeric version alone cannot distinguish beta iterations |
| Executable selection | Explicit packaged child paths; no implicit reuse of stable overrides |

The version-independent ongoing root should be `<KnownFolder LocalAppData>/AbigailPreview/profiles/ongoing`, or an explicit user-selected alternative validated against production overlap. The launcher resolves that absolute preview location and supplies it; daemons must never substitute the normal KnownFolder data path. No actual directory at that location was read or created for this proposal.

Fresh-creation fixture roots are separate unique directories beneath the test workspace, such as `<workspace>/target/preview-fixtures/<run-id>/creation`. Continuing-profile upgrade/reopen tests use a distinct `<run-id>/ongoing` fixture retained across candidate updates. Within-profile disposable-Entity tests remain useful, but reset/cleanup targets must never be the continuing root. Synthetic harnesses must require their explicit fixture root, not use the persistent launcher location.

The marker's schema is independent of Entity config schema 27. Persist the profile UUID across versions. Initial marker creation is allowed only in a new empty validated root; a populated unmarked root requires an explicit import decision.

After marker verification, the app supplies `ABIGAIL_PROFILE_ID=<uuid>` to its children. For a new Hive launch it generates `ABIGAIL_HIVE_INSTANCE_ID=<fresh-uuid>`; the daemon reports that exact instance in its descriptor/status, and Entity children receive it with the explicitly selected Hive URL. Profile/root/instance values must be verified before adopting an existing process or sending close/recovery requests. These non-secret identifiers identify ownership; they are not provider credentials. Never trust inherited `ABIGAIL_HIVE_URL` or executable overrides as ownership proof. Reject a conflicting inherited value rather than silently rebinding.

The assigned packaging worker owns Tauri packaging configs/overlays, `scripts/stage_split_installer_resources.ps1`, `scripts/build_signed_installer.ps1`, and release tests. This app worktree will not edit those files. Application engineering owns Rust config/profile/path/process code and app continuity tests. Coordinate the two Tauri compilation configurations: current staging uses one shared `TAURI_CONFIG` during a multi-package build, so a shared override cannot safely assign different Hive/Runtime identifiers. Preview overlays should preserve stable defaults.

The parent selects the integration source. Current app and release branches are main-based; selected beta Genesis/static-runtime/installer-lock behavior still needs deliberate reconciliation with main signing/provenance safeguards. No merge or source selection is implied by this comparison.

## Smallest implementation sequence

1. **Config refusal:** validate the serialized version before deserializing/migrating, guard saves against a newer on-disk target, and stop active callers from substituting defaults or consuming a backup after refusal. Test exact preserved bytes with synthetic files. This is the bounded first application change; schema 27 remains current.
2. **Startup/profile context:** validate compiled mode, root, marker schema and all relevant existing configs before logging, vault/DB initialization, WebView creation, descriptor access, or requests. Use one immutable context for every path listed in [PREVIEW_PROFILE_CONTRACT.md](PREVIEW_PROFILE_CONTRACT.md). Validate restored absolute paths before use. Preserve normal production defaults.
3. **Credential and connection dependencies:** pass a profile-specific provider through all master-key/vault/encrypted-storage constructors, with fake-adapter tests first. Bind channel/profile/root/live instance through daemon discovery, child launch, saved creation recovery, and close/restart. Remove preview fallback on failure. Steps 2–3 must be complete before enabling normal preview startup.
4. **Full cold snapshot/restore:** quiesce owners; inventory all persistent files and required credential availability; restore into a new fixture; rebase approved paths, invalidate descriptors, and verify the same UUID, history and settings. Existing per-Entity restore generates a new UUID and does not satisfy this requirement.
5. **Exact candidate acceptance:** coordinate distinct compiled/installer identities and run the ten stages against the final signed installer bytes. Preserve the accepted artifact; component tests cannot transfer acceptance to a rebuilt installer.

## Release-stage evidence mapping

| Exact stage ID | Application contribution / remaining evidence |
| --- | --- |
| `separate-installer-identity` | Proposed identities above; compiled and generated installer identities still require implementation and inspection. |
| `stable-preview-coexistence` | Requires isolated synthetic Windows install/upgrade/uninstall in both launch orders; no personal-profile run. |
| `profile-path-isolation` | Source path inventory and contract exist; complete context plumbing and sentinel-file tests remain. |
| `daemon-ownership-isolation` | Selected-Entity display guard exists; channel/profile/live-instance ownership, process lifecycle and port-race tests remain. |
| `fresh-creation-fixture` | Keyed creation component tests exist; use a separate fresh installed fixture for the existing daemon contract. |
| `ongoing-entity-upgrade` | Synthetic identity/profile preservation tests exist. First preview needs an explicit seeded baseline source/hash, not an invented prior accepted release. |
| `ongoing-entity-reopen` | UI restoration and four-phase scoped-store subprocess evidence exist; actual Hive/Runtime restart on the final candidate remains. |
| `full-cold-backup-restore` | No full snapshot/restore implementation or candidate acceptance is claimed. |
| `newer-schema-refusal` | Bounded config/caller checks are tracked below the whole-profile and installed acceptance level; startup preflight remains. |
| `return-to-stable` | Stable stays on its own unchanged profile; preview rollback requires a separately preserved compatible snapshot. |

Exact config refusal results are recorded in [schema-refusal validation](../output/playwright/preview-schema-validation.md). No real credential operation, daemon/installer launch, release publication, main/beta merge, or real-data migration is part of this bounded change.
