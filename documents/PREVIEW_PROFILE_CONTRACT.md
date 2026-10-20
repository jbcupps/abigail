# Persistent preview profile boundary

Application implementation status, 2026-10-20. The accepted profile contract is implemented in the current development source. The [previous isolation slice's component validation](../output/playwright/preview-profile-isolation-validation.md) records 167 selected Rust tests, 373 frontend tests, both frontend builds, and independent review. Those historical results do not validate subsequent cold-recovery changes. Recovery was published at `4558fe68d9812998eb53a94ba157970af552f923` and is included in the combined dev candidate. Its prior [component validation](../output/playwright/cold-profile-validation.md) passed 104 distinct Rust tests—101 core and three actual SurrealKV fresh-process recovery tests—and seven separately counted CLI smoke checks, with three ignored child fixtures and no unresolved blocking review findings. The archived record's publication-pending wording describes its earlier checkpoint; current combined checks are [recorded separately](../docs/PREVIEW_COMBINED_VALIDATION.md). Neither record establishes installed acceptance. No normal preview startup, real profile migration, OS credential operation, or signed-installer acceptance is claimed by this document.

The goal is one persistent preview profile containing an ongoing Entity and separate disposable Entities. Updating the preview application must preserve that profile. Stable defaults and existing stable data remain unchanged. A preview profile belongs to a selected beta release, not a third release channel.

The [release alignment](PREVIEW_RELEASE_ALIGNMENT.md) records the exact launcher contract, packaging ownership, and ten required installer acceptance stages. The bounded [Windows cold-profile recovery slice](PREVIEW_COLD_RECOVERY.md) is now implemented in source; final component and installed acceptance remain separate.

## Implemented application boundary

`abigail-core::profile` supplies one immutable `ProfileContext` per preview process. The four active entry points validate it before logging, WebView creation, credential loading, database initialization, descriptor adoption, or requests. A compiled-preview Hive launcher supplies only absent mode/root defaults; blank, invalid, or conflicting values fail. Daemons additionally require explicit matching arguments.

Preflight validates an absolute local root, existing path components, production-path overlap, marker identity/schema, active configs, and registry paths. It refuses symlinks/reparse points rather than following them. Existing profiles are inspected without migration. A new marker is published atomically without replacing an existing marker, only after validating an empty root. A populated unmarked root requires explicit import; a child cannot initialize a missing parent marker.

The marker at `R/profile.json` contains `schema_version: 1`, `kind: "preview"`, `channel: "beta"`, and a persistent non-nil canonical `profile_id` UUID. Profile schema 1 and Entity config schema 27 are separate contracts. Startup rejects unsupported marker/config schemas before startup consumers initialize. Active configs are the root config and `identities/<UUID>/config.json`; foreign WebView, skill, and archived `config.json` files are not interpreted as active Entity configs. Explicit restore validates a selected backup before consuming it.

| Source boundary | Implemented preview behavior |
| --- | --- |
| `abigail-core` config and registry | Derive defaults from the validated root. Validate saved paths before load/migration/save. Bind an active Entity config's data directory to its actual UUID directory so it cannot select another Entity's paths. |
| `abigail-identity` | Capture the context before vault initialization. Use UUID-scoped Documents and root-only backup discovery/output. Validate restored configuration before copying or registering it. Preview refuses implicit legacy root-identity migration. |
| `abigail-runtime` | Filesystem-tool allowances contain only the selected Entity's preview Documents and temporary directories; OS Documents and OS temporary roots are absent from preview allowances. |
| `abigail-memory` and Entity backup operations | Revalidate archive destinations and selected backup paths before I/O. Preview discovery stays under `R/backups`. |
| `abigail-diag` and native apps | Use explicit profile log paths and separate Hive/Runtime WebView data directories. |
| Vault, master-key and encrypted-storage callers | Use a profile-specific credential namespace and root-bound cache. Fake backend seams allow tests without calling the OS keyring or DPAPI. |
| Hive/provider setup | Reject ambient API-key and installed-CLI account reuse in preview. Explicitly configured preview credentials or local URLs remain the setup path. |
| Native apps, daemon clients and supervisor | Verify channel, profile UUID, canonical root and live process instances before adoption/use. Pin HTTP ownership headers and fail closed when native verification fails. |
| `profile_lease`, `profile_snapshot` and `abigail-profile` | Hold process-use leases in all four preview processes. Cold snapshot/restore requires exclusive cold/Hive ownership, inventories the whole retained root, verifies hashes, preserves UUIDs and requires existing credentials before publishing a fresh recovery root. Synthetic component validation is complete. |

## Paths

All paths below derive from the same validated root `R`:

| Purpose | Preview location |
| --- | --- |
| Registry, master key and vault files | Existing filenames beneath `R` |
| Entity configuration, identity and presentation | `R/identities/<entity-uuid>` |
| Entity internal identity documents and models | `R/identities/<entity-uuid>/docs` and `/models` |
| Shared conversation store | `R/memory.db` |
| Entity user Documents and filesystem-tool allowance | `R/Documents/<entity-uuid>` |
| Entity archives | `R/archives/<entity-uuid>` |
| Backup discovery/output | `R/backups` only |
| Runtime descriptors, ownership lock and sessions | `R/runtime` |
| Logs | `R/logs` |
| Tool temporary files | `R/tmp/<entity-uuid>` |
| Hive and Runtime WebView data | Separate role directories beneath `R/webview` |

If supplied, `ABIGAIL_DOCUMENTS_DIR` must equal `R/Documents`. Copied absolute config paths are validated before use; they cannot silently redirect a preview to stable files or another Entity. Existing stable Documents are not moved. UUID-based preview Documents avoid same-name collisions. Existing experimental roots need an explicit backed-up migration decision.

## Credentials and providers

Preview credential namespace `preview.<profile_id>` is used in keyring service/account selection and encrypted scope labels. The unlock cache also binds the canonical root, so a copied UUID does not make a different root the same cache entry. Preview DPAPI fallback is root-relative and includes its namespace in the protected payload; raw legacy fallback bytes and another profile's payload are refused. Stable account names and legacy behavior are retained for stable mode.

Preview IdentityManager refuses automatic identity reset after decryption failure. It also rejects a missing master key when a registry or existing identity files are present, before its own unlock/bootstrap operations. Fresh empty profiles remain eligible for initialization; these guards do not establish whole-process OS credential acceptance.

Preview does not import legacy credentials or read ambient raw-key/passphrase fallbacks. It also does not silently use ambient provider API keys or installed CLI sessions. The fake credential tests exercise namespace selection, cache separation, fallback handling and ciphertext separation; they do not validate Windows Credential Manager or DPAPI integration.

No cryptographic key is derived from the public profile UUID alone. A whole-root file copy does not prove the encryption key will be available at recovery time. Credential availability and actual OS integration remain separate acceptance work.

## Process and connection ownership

The launcher passes explicit packaged executable paths and profile/root/instance context. Preview endpoints use verified loopback URLs outside stable's reserved ports `43141..43150`. Hive holds an OS file lock for the selected profile before opening the store. The supervisor uses owned child handles; a stale descriptor PID never authorizes termination.

All four current preview processes also retain a shared process-use lease until exit. Cold operations require exclusive participation and Hive owner locks; restore locks the recorded original root if it still exists with the matching marker. This does not detect unknown clones, guarantee exclusion of arbitrary older/foreign writers, or prevent separate roots being reopened later. See the [cold ownership limits](PREVIEW_COLD_RECOVERY.md#cold-ownership-and-publication).

Status and descriptor `process_identity` values contain `channel`, `profile_id`, `canonical_data_root`, `instance_id`, and `hive_instance_id`. Callers compare them with the intended live owner. Verified clients pin `x-abigail-profile-id` and `x-abigail-process-instance-id`. Preview middleware requires these except for GET health/status and OPTIONS; current stable middleware rejects preview ownership headers. These non-secret fields guard accidental routing and stale ownership. They are not local-client authentication or executable provenance.

Native transport resolves and revalidates IPC metadata before HTTP/SSE use. A failed native handshake never falls back to a browser query, environment URL, or stable default port. The Runtime also requires the selected Entity ID. Startup/cleanup failure is visible in the UI with connection actions unavailable.

Preview shutdown requires acknowledgement that owned Entity children were drained/reaped and then the owned Hive child must exit. An uncertain drain does not authorize force-killing Hive or launching a replacement owner. Installer and actual process-lifecycle acceptance still need to exercise these source contracts.

Creation lookup, replay and profile save use the same verified transport. If a retained creation-journal URL differs from the pinned endpoint, recovery fails explicitly and preserves the exact creation key. Automatic same-profile endpoint rebinding is not implemented. Never replace an uncertain attempt with a fresh key or silently retarget it.

## Cold recovery and remaining acceptance

- The [previous isolation results](../output/playwright/preview-profile-isolation-validation.md) cover preflight rejection, sentinel preservation, UUID path scopes, fake credential calls and ownership checks. They do not substitute for final cold-recovery validation or the exact installed candidate.
- The new [cold recovery CLI](PREVIEW_COLD_RECOVERY.md) snapshots the complete `memory.db`, registry/receipts, identity, Documents and WebView data; excludes `runtime`, `tmp`, `logs` and `backups`; and publishes only a validated fresh destination. It rebases only declared paths and probes the existing sentinel/master-key credentials without bootstrap. Hashes provide integrity comparison, not authentication or bundle encryption.
- The published recovery slice's core and actual SurrealKV fresh-process selections passed 104 distinct Rust tests plus seven CLI smoke checks in the [prior component record](../output/playwright/cold-profile-validation.md). Combined reruns are recorded separately. The synthetic store/fake-credential harness does not establish GUI reopening or real provider/OS credential acceptance. Existing per-Entity restore still creates a new UUID and is unchanged. The offline recovery utility remains separate from the four packaged application binaries.
- Exercise OS credential integration using a reviewed isolated test plan, without the operator's existing credentials. See [STARTUP_TEST_ISOLATION_PLAN.md](STARTUP_TEST_ISOLATION_PLAN.md).
- Verify all ten [release stages](PREVIEW_RELEASE_ALIGNMENT.md#release-stage-evidence-mapping) against the final signed installer bytes. Fresh creation uses a separate fixture from the continuing-profile upgrade fixture. Keep old/new owners from opening one store concurrently.

The packaging worker owns application identifiers, overlays, staging, signing and installer tests on a separate branch. This application work does not alter those files, promote main/beta, or establish acceptance for an ongoing real profile.
