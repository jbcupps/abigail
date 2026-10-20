# Cold-profile recovery: final component validation

Recorded 2026-10-20 for the bounded cold-recovery slice on `dev/first-signal-integration-0.0.76`. **Final selections passed 104 distinct Rust tests: 101 core and three actual SurrealKV store/recovery tests, with three standalone child fixtures ignored. Seven CLI smoke checks also passed, counted separately.** The final core rerun after a formatting-only correction replaces the earlier execution of those same 101 tests. Component validation and independent review are complete; source commit/push and remote verification remain pending. The source revision is to be recorded by the commit containing this evidence file.

The previous [profile-isolation validation](preview-profile-isolation-validation.md) remains historical evidence for its recorded source: 167 selected Rust passes, 373 frontend passes and two frontend builds. Those results are not new cold-recovery runs and must not be added to this slice's totals.

## Scope

The [cold recovery guide](../../documents/PREVIEW_COLD_RECOVERY.md) records the implemented Windows same-user/device snapshot, verify and restore contract. Source preserves the profile/Entity UUIDs, full shared `memory.db`, exact creation keys and receipts, Documents, presentation and opaque WebView data. It excludes `runtime`, `tmp`, `logs` and `backups`, rebases only declared application paths, and requires existing credential material before publishing a fresh destination.

The four current preview processes retain shared leases. Cold operations require exclusive participation and Hive ownership; restore also locks the recorded original source when it still exists with the matching marker. Windows read guards refuse incompatible file writers. These mechanisms do not discover unknown clones, exclude every older/foreign writer, or permanently prevent two separate restored roots from being opened later.

All runs in this checkpoint are synthetic fixtures. No real profile, provider conversation, OS keyring/DPAPI integration, normal daemon/GUI startup, installer or release is accepted here.

## Completed evidence

The completed core results below were read from the actual six final result logs and checked against their [execution summary](cold-core-results.txt). The 101 passing test names are distinct across those selections. Their counts supersede earlier snapshot attempts rather than being added to them.

| Check | Recorded status | Evidence |
| --- | --- | --- |
| Snapshot/state, `profile_snapshot::` | 29 passed | [Snapshot/state tests](cold-core-snapshot-tests.txt) |
| Process leases, `profile_lease::tests::` | 9 passed; 1 standalone child fixture ignored | [Lease tests](cold-core-lease-tests.txt) |
| Existing-credential restore, `cold_restore_credentials_` | 7 passed | [Restore credential tests](cold-core-credentials-tests.txt) |
| Profile regression, `profile::tests::` | 14 passed; 1 standalone child fixture ignored | [Profile regression](cold-core-profile-regression-tests.txt) |
| Config regression, `config::tests::` | 37 passed | [Config regression](cold-core-config-regression-tests.txt) |
| Credential isolation regression, `preview_credentials_` | 5 passed | [Credential regression](cold-core-credential-regression-tests.txt) |
| Final current-source core test-harness build | Completed in 6.10s; actual test executable and successful build record verified | [Build log](cold-core-build.txt), [artifact metadata](cold-core-build-proof.txt) |
| Actual SurrealKV fresh-process store/recovery tests | 3 passed; 1 standalone child fixture ignored; 12.62s | [Store/recovery tests](cold-store-tests.txt) |
| Store test-harness build | Completed in 11m 13s; actual test executable and successful build record verified | [Build log](cold-store-build.txt), [artifact metadata](cold-store-build-proof.txt) |
| `abigail-profile` CLI build | Completed in 54.55s; actual executable and successful build record verified | [Build log](cold-cli-build.txt), [artifact metadata](cold-cli-build-proof.txt) |
| CLI help/parser/missing-snapshot checks | 7 passed with expected exit codes; separate from Rust test totals | [Results and all seven captured outputs](cold-cli-smoke.txt) |
| Changed Rust source formatting | Ten files checked; exit 0 | [Exact file list and command](cold-source-format-proof.txt) |

The completed store fixture seeded, snapshotted, restored and reopened the actual embedded SurrealKV store in separate test-harness processes with fake credentials. Its output contains 15 phase records with 15 distinct process IDs. Each of its three reopen records verifies two preserved Entity IDs, three store scopes, four exact conversation turns, three creation receipts and two Entity signatures. The fixture also checks scoped jobs/memories, master-key identity, presentation, Documents, archives and retained synthetic journal bytes. These counts describe each fixture's preserved contents; repeated reopen records are not extra test cases.

The three tests cover complete recovery, missing/wrong credentials with bundle/destination preservation, and interrupted restore followed by successful retry. The interruption intentionally exits the owned child with code 73 while staging; it exercises abrupt process exit, not power loss or storage-device failure. The retained partial directory is not published as the recovery destination. This remains a synthetic test-harness result, not GUI reopening or a real WebView-store compatibility run.

The core build proof retains the actual Cargo `compiler-artifact` record for `abigail_core`: `profile.test` is true and the emitted executable is under `target/debug/deps`. The final Cargo `build-finished` record reports success; all six selected executions have exit code zero. The store build proof likewise retains the actual `cold_profile_restore` test artifact and successful final build record. The CLI proof is the regular `abigail-profile` executable with `profile.test: false`; its smoke checks are not Rust harness tests. The three ignored tests across core/store are subprocess support fixtures, not additional passes.

The CLI checks cover root help, three subcommand help screens, missing required snapshot arguments, and verify/restore refusal of a nonexistent snapshot. The four help cases exit 0, missing arguments exit 2, and both nonexistent-snapshot cases exit 1 as expected. The smoke runner checks that the refused restore destination is absent before writing its successful results. Restore rejects the missing bundle before reaching credential probing. These checks do not perform a successful CLI restore using real credentials.

The nineteen durable TXT files linked here strip ANSI sequences, replace absolute repository/workspace/user paths with placeholders and trim trailing whitespace. No credential values were observed. Build proofs retain selected actual Cargo records with dependency records omitted. Full historical failure diagnostics are preserved separately from passing results; no large compiler argument line needed truncation in these cold-slice logs.

## Commands and execution boundaries

`TEMP` and `TMP` pointed to the repository's `target/cold-profile-tmp` fixture directory. The recorded build commands were:

```powershell
cargo test --locked --offline -j1 -p abigail-core --lib --no-run --message-format json 1> target/cold-core-build.jsonl 2> target/cold-core-build.log
cargo rustc --locked --offline -j1 -p abigail-persistence --test cold_profile_restore --profile test --message-format json -- -C link-arg=/DEBUG:NONE 1> target/cold-store-build.jsonl 2> target/cold-store-build.log
cargo build --locked --offline -j1 -p abigail-profile --message-format json 1> target/cold-cli-build.jsonl 2> target/cold-cli-build.log
```

Each executable was selected from its matching actual Cargo artifact record after verifying build success. The core executable ran each of the six table filters with `--test-threads=1 --nocapture`; the store test executable ran with `--test-threads=1 --nocapture`. The store build's linker flag was invocation-specific and did not change persistent build settings or assertions. The CLI ran `--help`, `snapshot --help`, `verify --help`, `restore --help`, `snapshot`, `verify --snapshot <missing-fixture-path>`, and `restore --snapshot <missing-fixture-path> --destination <fresh-fixture-path>`.

The exact ten-file `rustfmt --edition 2021 --check --config skip_children=true` selection is preserved in the format proof. The working-tree `git diff --check` also passed; the final staged check belongs to publication.

## Earlier failures and corrections

- The [initial compile check](cold-initial-sha2-compile-failure.txt) failed with `E0277`: the SHA-2 output array did not implement `LowerHex` for `format!("{:x}", digest.finalize())`. Source was corrected to encode individual digest bytes. That initial check did not execute tests.
- The [first snapshot run](cold-snapshot-initial-failures.txt) recorded 24 passes and two failures. One assertion compared equivalent Windows paths as strings, with and without the canonical `\\?\` prefix; the test comparison was corrected. The other failed during fixture snapshot publication with Windows access denied (`0x80070005`).
- A [later snapshot attempt](cold-snapshot-publication-failure.txt) recorded 28 passes and one failure during `MoveFileExW` publication, again with access denied. The failure occurred while preparing a snapshot for a manifest-refusal test. The log does not prove which external Windows component caused the denial.
- Reviewed Windows publication source permits at most four attempts, only for access/sharing errors when the staging source still exists and the destination remains absent. It verifies unchanged canonical paths before retrying a known-unpublished move, never overwrites a destination and never changes permissions. The final current-source snapshot selection records all 29 passes. Earlier attempts remain diagnostic history and contribute no extra passed-test count.

Incremental-cache hard-link fallback warnings in these logs are distinct from test failures.

Independent review closed without unresolved blocking findings in this bounded Windows cold-recovery slice. Addressed corrections include rejection of declared secrets paths inside excluded subtrees, rejection of canonical partial-directory bundles during verification, and bounded no-overwrite publication retry. These are source/component findings, not installed acceptance.

## Remaining acceptance

Only source publication and remote verification remain pending for this component record. The completed synthetic fresh-process store tests and CLI smoke checks do not establish existing Windows Credential Manager/DPAPI behavior, real provider accounts, normal GUI/daemon lifecycle, or recovery using the exact signed installer. Every installed acceptance stage remains open to its own evidence; no installer stage is promoted by this record.

Read-only review of `ca6a` identified existing preview list/get/update guards for future or corrupt configs. Forge's claim concerned older Linux source and is not a fresh reproduced Windows defect in this record. No fresh HTTP proof is asserted, stable metadata fallback is a separate behavior, and status metadata does not attest current disk contents.

Archived isolation records, packaging handoffs and packaging-owned scripts/configs are unchanged by this documentation update.
