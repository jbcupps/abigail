# First Signal startup test isolation proposal

Source inspection only, 2026-10-20, on the First Signal integration based on main `17b636b`. No proposed credential-related action below has been executed. This plan does not change credential protections or authorize access to an existing user's credential store.

## Why full daemon acceptance remains held

The proposed HTTP acceptance would start newly built Hive and Entity daemons with temporary data and Documents roots, synthetic provider responses, and a synthetic raw vault key. It would exercise keyed creation/replay, profile save, quickstart birth/signing, chat/SSE, history, reopen, and process restart. The draft `scripts/tests/first-signal-acceptance.mjs` currently refuses `--run`.

Normal startup is not isolated by those environment settings:

- `crates/hive-daemon/src/main.rs:72-77` resolves the data directory and calls `IdentityManager::new`.
- `crates/abigail-identity/src/lib.rs:203-206` creates `HybridUnlockProvider` and calls `root_kek()`.
- `crates/abigail-core/src/vault/unlock.rs:115` calls `os_keyring_load_optional()` on a fresh process before trying the raw-key environment override at line 139 or passphrase at line 148.
- `unlock.rs:346-350` constructs a keyring entry and calls `get_password()`. A successful keyring load may also persist the DPAPI fallback. This is a credential-store operation even when all Entity data paths point to temporary directories.
- `crates/abigail-core/src/keyring.rs:188,205` independently constructs the hybrid provider for master-key generation/loading.
- `crates/abigail-identity/src/lib.rs:578` reaches hybrid-backed encrypted storage during birth/recovery work. Injecting only the first constructor would not isolate the whole flow.
- `crates/entity-daemon/src/main.rs:218` loads its skill vault; startup also initializes vault state and calls `IdentityManager::new` again at line 466.

The existing `crates/hive-daemon/tests/startup.rs` sets `ABIGAIL_VAULT_RAW_KEY` at line 28 and starts the normal executable at line 115. That test does not prove zero credential access and was not run for this integration.

## Existing dependency injection seams

- `UnlockProvider` is a `Send + Sync` trait at `crates/abigail-core/src/vault/unlock.rs:69`. A test-local implementation can return fixed, explicitly synthetic bytes without reading environment secrets or calling an OS API.
- `SecretsVault::open_with_provider` at `crates/abigail-core/src/secrets.rs:100` retains the supplied provider for later saves.
- `ScopedVault::open` at `crates/abigail-core/src/vault/scoped.rs:57` accepts that provider.
- `isolated_creation_test_manager` at `crates/abigail-identity/src/creation.rs:80` constructs real registry state with a fixed synthetic signing key. It deliberately skips normal master-key/bootstrap and Hive-helper provisioning. The feature is enabled only by the daemon test dependency and absent from the verified normal identity build.
- The private process-global session-key cache at `crates/abigail-core/src/vault/mod.rs:29` is not a supported injection interface and should not be manipulated to avoid the normal path.

## Proposed next test-only slice

1. Extract or expose only the application-state/router assembly needed for an in-process test. Keep normal executable bootstrap and helper spawning outside that harness.
2. Supply the existing isolated manager plus `SecretsVault::open_with_provider` instances sharing one test-local fixed provider. Use unique temporary directories below the worktree.
3. Dispatch actual HTTP request objects through the assembled router. Exercise readiness, keyed creation, profile persistence, errors, and reload through real storage.
4. Record exactly which bootstrap services were substituted. Do not label these results as production startup, birth/signing, real credential integration, desktop, or full child-process acceptance.
5. Keep the normal-binary HTTP harness gate closed. No new environment override, user setting, cache priming, or keyring fallback change is needed for this narrower test.

This is ordinary test dependency injection and can improve coverage without accessing real credentials. It is a proposal, not completed validation.

## Separate full-bootstrap refactor

Full synthetic normal-bootstrap coverage would require explicitly passing a provider through identity initialization, master-key generation/loading, and every relevant encrypted-storage operation. Entity/helper processes need their own bounded test entry points; parent-process injection cannot cover a new process automatically. Review that concrete design and all reachable constructors before running it.

Such a refactor should preserve the production provider selection, ordering, access checks, and failure behavior. It should prove that the synthetic test entry point cannot select the actual OS store. It would still not validate production OS credential integration or real-provider inference; those are distinct acceptance activities requiring their own scope and authorization.
