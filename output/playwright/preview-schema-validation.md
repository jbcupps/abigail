# Preview schema-refusal component validation

2026-10-20. Application branch `dev/first-signal-integration-0.0.76`, extending `d883fe0`. Release scaffolding `6b246d3` stays separate. This is component evidence for the release contract's `newer-schema-refusal` requirement, not completion of the exact-installer stage.

## Implemented behavior

- `AppConfig::from_json_compatible` validates the schema header before deserializing fields, without migration or I/O. Load uses it before migration.
- Save rejects a future in-memory schema before creating a target. A stale config cannot overwrite a newer or invalid schema header already on disk. Malformed, duplicate, overflowing and nonnumeric schema values are refused.
- Entity Runtime config loading defaults only when its config path is actually missing. Existing config errors propagate instead of starting with replacement defaults.
- Reserved creation replay validates compatibility without migrating or changing saved config bytes.
- Per-Entity backup restore refuses an incompatible or invalid existing config before allocating/moving a restored identity. This does not implement complete profile backup/restore or change its existing new-UUID semantics.

Schema 27 and supported migration transformations remain unchanged. The save guard is sequential protection under the existing single-owner assumption, not a cross-process compare-and-swap.

## Scope

All tests use invented files in workspace-owned temporary directories and the existing inert IdentityManager fixture where needed. Tests do not run daemon `main`, access a real credential store, invoke providers, use real Entity data, or execute an installer.

Actual command output is sanitized for machine paths, terminal control sequences and trailing whitespace. UI code did not change in this follow-up, so the previous 343 UI tests/build results are not presented as newly rerun.

## Completed checks

| Command from repository root | Result | Evidence |
| --- | --- | --- |
| `cargo test --locked --offline -p abigail-core -j 1 --lib config::tests` | 37 pass: 32 AppConfig and five matching GlobalConfig tests | [Core tests](preview-schema-core-tests.txt) |
| `cargo test --offline -p abigail-identity --lib creation::tests -- --nocapture` | 22 pass, including four new compatibility tests | [Identity tests](preview-schema-identity-tests.txt) |
| `cargo test --offline -p abigail-identity --lib creation::tests::restore_ -- --nocapture` | Both restore tests pass again after the final scope correction | [Final restore tests](preview-schema-identity-final-restore.txt) |
| Verified Cargo test-profile artifact, `config_loading_tests --test-threads=1` | Five pass, 29 filtered out; no daemon `main` executed | [Entity config tests](integration-config-loading-entity-final-tests.txt) |

The three suites total **64 unique passing tests**; the final two restore checks are a subset rerun. The Entity test executable was built using `cargo rustc --locked --offline -j 1 -p entity-daemon --bin entity-daemon --profile test --message-format json -- -C link-arg=/PDB:NONE`. Cargo reported the `entity-daemon` artifact with `profile.test=true` and build exit 0; listing the selected tests confirmed the five helper tests before execution. [Successful build output](integration-config-loading-entity-final-build.txt).

The initial regular test build failed at linking with [LNK1140](integration-config-loading-entity-first-attempt.txt), before any test ran. The first command-local linker override then failed with [insufficient disk space](integration-config-loading-entity-relink.txt). After reclaiming only verified generated PDB/incremental artifacts in this worktree, the final build and test run succeeded. The override changed no repository or persistent toolchain setting. On this toolchain it produced a generated PDB named `NONE`, which was removed after validation; it did not disable PDB generation.

The identity suite's two new refusal regressions were first run against the original caller behavior and both failed as expected: a reserved newer config was accepted and an unsupported backup was consumed. [Captured failing baseline](preview-schema-identity-before.txt). Their corrected results above prove that the tests detect the changed behavior.

Independent source review found no actionable new regression after the scope correction. Rustfmt checks and `git diff --check` passed. Cargo.lock is unchanged.

## Remaining release acceptance

Normal bootstrap still initializes logging and other state before reaching individual config loaders. A complete profile context and preflight must reject unsupported profile/config versions before any initialization writes. Status/listing paths can also suppress config-load errors; they are not proof of a compatible profile. No normal daemon startup was attempted to bypass those limits.

The existing per-Entity restore operation is not transactional after moving a supported backup; its config save remains best-effort. An initially added propagation of that save error would have introduced an unregistered moved-directory failure, so it was removed during review. This slice guarantees pre-move refusal of incompatible input, not rollback for later I/O failures. Complete cold profile restore requires separate implementation and acceptance.

Compiled preview identity, all path consumers, separate credential namespace, channel/profile/live-instance ownership, cold snapshot/restore and signed-candidate acceptance remain open. The exact stage mapping and proposed build/config parameters are in [PREVIEW_RELEASE_ALIGNMENT.md](../../documents/PREVIEW_RELEASE_ALIGNMENT.md).

No release workflow, installer script, Tauri identifier, dependency lockfile, main/beta branch, public preview descriptor or real profile was changed. No browser action was retried; the previously reported viewport approval uncertainty remains.
