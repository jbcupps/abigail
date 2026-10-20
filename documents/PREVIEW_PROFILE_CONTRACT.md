# Proposed persistent preview profile boundary

Source review and packaging contract proposal, 2026-10-20. **This contract is not implemented yet.** Setting the variables below on the current application does not establish the proposed isolation. No real user data, credentials, profile directories, or release routing were changed during this review.

The goal is one persistent preview profile containing an ongoing Entity and separate disposable Entities. Updating the preview application must preserve that profile. Normal production behavior and existing production data must remain unchanged.

## Current paths that prevent an isolation claim

| Source | Current behavior |
| --- | --- |
| `hive-app/src/lib.rs:445,533` | `ABIGAIL_DATA_DIR` changes the runtime descriptor path and is forwarded as Hive `--data-dir`. Missing/empty overrides fall back to normal locations. |
| `crates/abigail-core/src/config.rs:524,589` | Active `default_paths` uses the platform application directory, not `ABIGAIL_DATA_DIR`. `trusted_config_path` defines the same fallback but has no active caller in the inspected source. Windows Known Folder resolution is not reliably redirected by changing `LOCALAPPDATA`. |
| `crates/abigail-identity/src/lib.rs:686-742` | A genuinely custom root uses `<root>/Documents`; `ABIGAIL_DOCUMENTS_DIR` can override it. The normal root uses the user's Documents. Subfolders use sanitized names, not Entity UUIDs. |
| `crates/abigail-runtime/src/lib.rs:339-346` | Native filesystem allowed roots include the OS temporary directory and normal `Documents/Abigail`, ignoring both data/Documents overrides. This grants reach outside a custom profile. |
| `crates/entity-daemon/src/main.rs:446-450`; `crates/abigail-memory/src/archive.rs:84-87` | When an Entity public key exists, archive startup uses OS Documents and creates `Documents/Abigail/archives`, regardless of the custom data root. |
| `crates/abigail-memory/src/backup_import.rs:116-123`; `crates/entity-daemon/src/backup_ops.rs:26` | Backup discovery scans both a data-root directory and normal `Documents/Abigail/backups`. |
| `hive-app/src/lib.rs:461`; `crates/abigail-diag/src/lib.rs:20` | Desktop/daemon logs have independent normal-profile defaults. |
| `hive-app/src/lib.rs:584-610`; both apps' `src-ui/src/lib/connection.ts` | The shell adopts a healthy recorded daemon without verifying a profile identity. A supplied `ABIGAIL_HIVE_URL` bypasses descriptor discovery. Spawn failure is logged without failing setup, and native connection resolution can fall back to query/environment overrides or normal default ports. |
| `hive-app/tauri.conf.json:4`; `entity-runtime-app/tauri.conf.json:4` | Current identifiers are production identifiers, with no explicit preview WebView storage root. The browser's creation journal/session hints need a stable, separate preview store too. Cached Tauri 2.11.3 exposes `WebviewWindowBuilder::data_directory` for an explicit absolute directory. |
| `crates/hive-daemon/src/birth.rs:123-129,180,196-197` | Birth writes through saved config paths. A copied config can still point outside a new profile even when daemon startup has a custom root. |
| `hive-app/src-ui/src/lib/presenceCreation.ts:122,160-181`; `components/FirstSignal.tsx:182-183` | Creation recovery and subsequent profile save can use the journal's retained `hiveUrl`, independently of initial shell endpoint discovery. |
| `crates/abigail-core/src/vault/unlock.rs:18-19,347,383`; `vault/mod.rs:29` | The OS keyring service/account and process-wide unlock cache are not keyed by profile. Root-relative vault/DPAPI files alone do not create a separate credential namespace. |
| `crates/abigail-hive/src/hive.rs:289-339` | Provider lookup can fall back to ambient API-key variables or installed CLI accounts. A separate local vault does not isolate those accounts. |

## Proposed configuration contract for packaging

1. A dedicated preview entry point/build requires preview mode, proposed as `ABIGAIL_PROFILE=preview`, and an absolute `ABIGAIL_DATA_DIR=<preview-root>`. The variable name is a proposal until application/release owners agree. Missing mode must not turn a preview binary into production mode. The default production entry point retains its existing behavior.
2. `<preview-root>` is stable across preview versions and independent of the versioned installation directory. It must not equal, contain, or sit inside normal production-owned data, Documents/Abigail, runtime, logs, or WebView storage directories. Resolve canonical/reparse-point identity before accepting it, including the existing parent of a new root. Missing, blank, relative, invalid, inaccessible, or production-overlapping roots stop preview startup before credential loading, default config loading, logging, WebView creation, descriptor access, requests, or helper launch.
3. An atomic `profile.json` marker stores a schema version, `kind: "preview"`, and a generated profile UUID. The UUID is a non-secret identity, never an encryption key. An existing populated directory without a valid matching marker requires an explicit migration/import decision; do not silently adopt it. Keep that UUID unchanged during updates.
4. Derive all preview-owned paths from the same immutable context:

   | Purpose | Proposed location |
   | --- | --- |
   | Registry, master key and vault files | Existing filenames beneath `<preview-root>` |
   | Entity configuration/identity/presentation | `<preview-root>/identities/<entity-uuid>` |
   | Shared conversation store | `<preview-root>/memory.db` |
   | Entity Documents and filesystem-tool allowance | `<preview-root>/Documents/<entity-uuid>` |
   | Entity archives | `<preview-root>/archives/<entity-uuid>` |
   | Backup discovery/output | `<preview-root>/backups` only |
   | Runtime descriptors and sessions | `<preview-root>/runtime` |
   | Logs | `<preview-root>/logs` |
   | Tool temporary files | `<preview-root>/tmp/<entity-uuid>` |
   | Hive and Runtime WebView data | Separate role directories beneath `<preview-root>/webview` |

   For preview, either derive `ABIGAIL_DOCUMENTS_DIR` internally or accept only the exact agreed preview Documents root. Never honor an outside override or fall back to OS Documents. Validate persisted path-valued settings before using them; copying an old config must not redirect the new profile to production files.
5. Propagate the same profile/root explicitly to Hive, its helper, each Entity process, and both native UIs; reject conflicting arguments or inherited values. Descriptor and status/connection handshakes verify the profile UUID and expected root ownership before endpoint adoption. A copied UUID alone cannot distinguish an active original from a restored root. Do not accept an arbitrary healthy production daemon, including through URL overrides. Propagate startup failure to the UI and disable native fallback to normal default ports or browser overrides. One immutable profile per process is the minimum cache/ownership model.

   Verify retained creation-journal endpoints before lookup, replay, or profile save too. Preserve the exact creation key; never replay it against a different profile. A changed port can be rebound only after verifying that the new endpoint owns the same intended profile/root.
6. Preview uses a distinct OS keyring account bound to its profile UUID. Production retains its existing account. Preview must not read, repair, fall back to, or migrate the production entry implicitly. Root-relative DPAPI fallback and every master-key/encrypted-storage helper must receive the same context. This requires coordinated dependency plumbing; changing only the first constructor is insufficient.
7. Provider reuse is explicit. Do not silently inherit production provider settings, ambient API keys, or CLI accounts. A user may separately choose to connect an existing provider account, with the same visible verification and data-flow explanation used by normal model setup.
8. Packaging supplies separate preview identity/shortcut/update routing and preserves this root and WebView data. Those release/channel changes belong to the release-engineering owner. Application code must enforce the boundary even when an environment variable or launcher argument is missing.

## Narrow implementation ownership

Use one shared profile/path context in `abigail-core`, then wire the active native apps, Hive/Entity startup and supervisor, runtime filesystem roots, archive/backup operations, diagnostics, and vault constructors through it. Do not add parallel ad hoc path fallbacks. Coordinate shared `main.rs`/status DTO changes with the release owner; leave release workflows and channel routing out of this application worktree.

For a fresh preview profile, UUID-based Documents avoid the inherited same-name collision. Do not move existing production Documents. Existing experimental roots need an explicit, backed-up migration plan rather than silently changing their path/key namespace.

## Required synthetic acceptance and migration evidence

- Pure path/context tests reject missing/relative roots, root overlap, mismatched markers, outside Documents/config paths, and a wrong-profile daemon. Invalid configuration must perform no fallback filesystem or credential operation.
- Use fake credential-provider/keyring adapters to prove namespace selection and absence of calls to the production account. Run no actual credential operation until the concrete plan is reviewed.
- Create a synthetic maintained Entity plus a disposable Entity. Exercise create, profile save, native tool root selection, archive destination, backup enumeration, reopen, and runtime restart. Sentinel files representing production data must remain byte-identical and absent from allowed roots/discovery results.
- Stop all owners before a consistent whole-root snapshot. Inventory external artifacts and credential dependencies too: current Documents/archive output, WebView state, and OS credential entries are not all inside the root. Even after the proposed path changes, a root copy alone does not establish availability of the required encryption key.
- Restore into an explicitly designated isolated recovery root. Rebase persisted absolute config paths through a reviewed import step or reject them; never follow paths back to the source profile. Invalidate copied PID/URL runtime descriptors. Keep the original profile stopped during recovery validation, then verify expected root ownership, profile identity, credential/decryption availability, registered UUIDs, exact committed transcript and presentation before allowing mutations. A file copy alone is not restore verification.
- Verify the actual signed candidate update preserves the same profile UUID/root and maintained Entity while its binary revisions change. Disposable Entity maintenance must not alter the maintained Entity. Do not run old/new application owners concurrently against one store.

These tests separate application profile isolation from real-provider inference, installed OS credential integration, and installer/channel acceptance. The latter remain separately scoped activities. The current First Signal source/tests do not yet satisfy this complete preview contract.
