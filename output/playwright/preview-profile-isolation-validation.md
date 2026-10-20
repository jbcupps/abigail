# Preview profile isolation: final component validation

Recorded 2026-10-20 from the development worktree on `dev/first-signal-integration-0.0.76`, following `fedd4210525711e414666d1326afc46a6604c395`. The validated source and this evidence file belong to the same development commit. The task handoff records its final revision and remote-hash verification; this file does not embed its own commit hash. All selected component checks are complete.

## Scope

The application now carries one validated immutable preview context through configuration, shared `memory.db`, identities, UUID-based user Documents, native tool roots, archive/backup paths, diagnostics, WebViews and credential selection. Startup preflight refuses incompatible schemas, invalid roots and redirected active configuration before initialization. Stable defaults are preserved.

Native apps and daemons verify profile/root/channel/live-instance metadata, pin ownership headers, and reject fallback after a failed native connection check. Creation recovery preserves the exact key and explicitly refuses a stale retained endpoint. Automatic endpoint rebinding is not implemented.

The exact build variables, launcher environment, daemon arguments, selected Entity context, ownership fields/headers, and shutdown acknowledgement are in the [accepted launcher contract](../../documents/PREVIEW_RELEASE_ALIGNMENT.md#accepted-applicationlauncher-contract). The [profile contract](../../documents/PREVIEW_PROFILE_CONTRACT.md) describes the source boundaries and remaining acceptance.

Only synthetic fixtures and fake credential adapters are used for this validation. Rust tests use explicit workspace temporary directories and offline locked dependency resolution. No normal GUI/daemon startup, existing profile, real conversation, OS keyring/DPAPI operation, installer execution or production deployment is part of this slice.

## Completed captured runs

These results were read from actual command output. They are separate selections, not a complete workspace test run. The final selections contain **167 distinct passing Rust tests**, plus one ignored standalone child fixture, and **373 passing frontend tests** across 18 files. Both UI builds passed. The [Rust count cross-check](preview-profile-final-counts.txt) records the 23 selected result logs; earlier attempts and repeated selections are not added to these totals. The core/config/credential/identity/creation and Hive daemon rows use the final completed reruns after their review corrections.

Rust commands use `--locked --offline -j1` and selected library tests with `-- --test-threads=1`, unless noted.

| Package / selected filter | Observed result | Captured output |
| --- | --- | --- |
| `abigail-core`, `profile::tests` | 14 passed; 1 standalone child fixture ignored | [Core profile tests](preview-profile-core-tests.txt) |
| `abigail-core`, `config::tests` (also selects matching global-config tests) | 37 passed | [Config tests](preview-profile-config-tests.txt) |
| `abigail-core`, `preview_credentials_` | 5 passed | [Fake credential tests](preview-profile-credential-tests.txt) |
| `abigail-identity`, `preview_profile_tests` (includes stable recovery regression) | 14 passed | [Identity tests](preview-profile-identity-tests.txt) |
| `abigail-identity`, `creation::tests` | 22 passed | [Creation tests](preview-profile-creation-tests.txt) |
| `abigail-diag`, library tests | 3 passed | [Diagnostics tests](preview-profile-diag-tests.txt) |
| `abigail-memory`, `preview_` | 6 passed, including one pre-existing matching test | [Archive/backup tests](preview-profile-memory-tests.txt) |
| `abigail-runtime`, `tool_roots` | 3 passed | [Runtime tool-root tests](preview-profile-runtime-tests.txt) |
| `abigail-hive`, `preview_provider_` | 5 passed | [Provider tests](preview-profile-providers-tests.txt) |
| `daemon-client`, `preview_tests` | 2 passed after linker-only retry | [Client tests](preview-profile-client-tests.txt) |
| `abigail-persistence`, preview transport tests | 3 passed | [Persistence transport tests](preview-profile-persistence-tests.txt) |
| `hive-daemon`, selected current-source test-harness groups | 26 passed: profile 5, identity/lifecycle 9, provider 1, shutdown 2, existing supervisor 9 | [Profile](preview-profile-hive-daemon-profile-tests.txt), [identity/lifecycle](preview-profile-hive-daemon-identity-tests.txt), [provider](preview-profile-hive-daemon-provider-tests.txt), [shutdown](preview-profile-hive-daemon-shutdown-tests.txt), [supervisor](preview-profile-hive-daemon-supervisor-tests.txt) |
| `hive-daemon`, final test-harness build | Completed in 19.24s; emitted test executable and successful build record verified | [Build output](preview-profile-hive-daemon-build.txt), [artifact metadata](preview-profile-hive-daemon-build-proof.txt) |
| `entity-daemon`, selected preview test-harness groups | 14 passed: startup 5, client 3, provider 2, binding 2, backup 2 | [Startup](preview-profile-entity-daemon-startup-tests.txt), [client](preview-profile-entity-daemon-client-tests.txt), [provider](preview-profile-entity-daemon-provider-tests.txt), [binding](preview-profile-entity-daemon-binding-tests.txt), [backup](preview-profile-entity-daemon-backup-tests.txt) |
| `entity-daemon`, test-harness build | Completed in 10m 26s; emitted test executable and successful build record verified | [Build output](preview-profile-entity-daemon-build.txt), [artifact metadata](preview-profile-entity-daemon-build-proof.txt) |
| Hive native library, `tests::` | 11 passed | [Hive native tests](preview-profile-hive-native-tests.txt), [execution summary](preview-profile-hive-native-results.txt) |
| Runtime native library, `tests::` | 2 passed | [Runtime native tests](preview-profile-runtime-native-tests.txt), [execution summary](preview-profile-runtime-native-results.txt) |
| Both native libraries, current-source test-harness builds | Hive completed in 7m 10s; Runtime in 7.92s; emitted test executables verified | [Hive build](preview-profile-hive-native-build.txt), [Hive artifact metadata](preview-profile-hive-native-build-proof.txt), [Runtime build](preview-profile-runtime-native-build.txt), [Runtime artifact metadata](preview-profile-runtime-native-build-proof.txt) |
| Hive UI, `npm test` | 281 tests, 11 files passed | [Hive UI tests](preview-profile-hive-ui-tests.txt) |
| Runtime UI, `npm test` | 92 tests, 7 files passed | [Runtime UI tests](preview-profile-runtime-ui-tests.txt) |
| Both UI packages, `npm run build` | TypeScript and Vite passed | [Hive build](preview-profile-hive-ui-build.txt), [Runtime build](preview-profile-runtime-ui-build.txt) |

The initial daemon-client test link failed with MSVC `LNK1140` while producing its program database, before any test executed. The retry used `cargo rustc --profile test --lib -- -C link-arg=/DEBUG:NONE`; the emitted artifact was checked to be a test harness and its selected preview tests were executed. The [initial failure](preview-profile-client-initial-link-failure.txt) retains the diagnostic and outcome, with the enormous compiler-emitted linker argument list omitted. This linker flag is invocation-specific; no persistent build configuration or test assertion was weakened.

The Entity daemon [artifact metadata](preview-profile-entity-daemon-build-proof.txt) is extracted from actual Cargo JSON records: `profile.test` is true, the emitted executable is under `target/debug/deps`, and `build-finished.success` is true. Its [execution summary](preview-profile-entity-daemon-results.txt) records five selected filters, their matched counts and zero exit codes. The same artifact checks passed for the final Hive daemon and both native libraries. Native test harnesses were compiled with `cargo rustc --profile test --lib -- -C link-arg=/DEBUG:NONE` and then executed. These results exercise compiled test harnesses; they do not establish normal daemon or GUI startup. The persistence selection validates pinned request construction and refusal without opening a socket.

The earlier Hive supervisor [restricted-loopback attempt](preview-profile-hive-restricted-loopback-first-attempt.txt) recorded eight passes and one failure in `readiness_rejects_another_entity_even_when_health_succeeds`. The root reported that attempt used restricted loopback access; the [exact same test subsequently passed](preview-profile-hive-readiness-network-retry.txt) after local access was permitted. Review then added actual-request counter assertions to two readiness fixtures so an unreachable test server cannot produce a vacuous rejection/redirect pass. The final current-source build and all five selected groups ran with approved local loopback access: the [execution summary](preview-profile-hive-daemon-results.txt) records 26 matches and zero exit codes. Those final 26 results supersede the earlier runs; the failure and retry remain historical diagnostics and contribute no extra test count.

The completed logs also contain incremental hard-link fallback notices and an existing unused-helper warning. These are not test failures. Duplicate UI log aliases are not additional runs or additional tests. The core startup-refusal tests invoke a dedicated child fixture; that support fixture is ignored as a standalone test and is not counted as another passed case.

The linked copies remove ANSI sequences, replace absolute repository/workspace/user paths with placeholders, and trim trailing whitespace. No credential values were observed. The initial linker failure explicitly marks its omitted long argument line. Build-proof files are labeled as selected actual Cargo records with dependency records omitted; separate build logs and test outputs are preserved. Superseded attempts are labeled as historical evidence, and no incomplete run is presented as passed.

## Review corrections

- Marker publication cannot replace a concurrently created owner. Existing marker/config bytes remain unchanged on rejection.
- Active config validation binds the serialized data directory to the actual Entity directory. A config for Entity A cannot coherently redirect all its paths to Entity B and then migrate through that directory.
- Startup distinguishes active Entity configs from unrelated WebView, skill and backup files named `config.json`.
- Preview legacy identity migration fails before consuming source files. Per-Entity restore rebases a declared optional public-key path even when the physical key is absent, so an unborn restored Entity is not registered with an unusable old path.
- Native transport verifies connection metadata before requests, bounds IPC failure, honors abort, and preserves the exact creation key on endpoint mismatch.
- Startup cleanup requires an acknowledged drain and owned-child exit before releasing ownership or replacing Hive. Selected supervisor launch/close/shutdown and native cleanup tests now pass.
- Review added refusal to replace missing or damaged master-key identity material in an existing preview profile. The final identity run passes missing-key byte-preservation and failed-decryption checks, alongside the stable recovery regression.

These are source/review findings, not proof that a signed installer has exercised them.

Separately relayed review comments have a narrower evidence basis: the parent reports that Atlas inspected older `fedd4210`/`6b246d3a` source without running tests. Its backup, creation-retention and schema observations are not current reproduced defects; complete-profile cold restore is already tracked as the next slice. Current keyed creation has the 22 passing tests linked above. A separate Claude review had no source access or test execution, so its claim that idempotency is absent is unsupported by that review and is not adopted here.

## Completed checks and publication status

All selected Rust and frontend tests, native/daemon test-harness builds and both UI builds are complete, with the final outputs linked above. The root also reports that the final changed-Rust `rustfmt` check and `git diff --check` passed. Earlier `cargo check --tests` was compile-only evidence and is not counted as test execution.

The development publication receipt belongs in the task handoff, after committing this source and evidence together. No publication hash is asserted here. The OS credential, full cold-restore and exact-installer acceptance boundaries below remain open.

## Screenshots and visual scope

Existing [integrated home](14-integrated-home.jpg), [recovered reveal](17-recovered-reveal.jpg), and [restored conversation](18-restored-conversation.jpg) images remain historical evidence for their earlier synthetic HTTP transport and recorded revision. They are not screenshots of a persistent preview profile, current native startup, or this isolation slice. Earlier images 01–13 are prototype evidence.

No new browser session or screenshots were captured here. An earlier viewport operation has an unresolved approval outcome and was not repeated. No current narrow-screen/mobile acceptance is claimed.

## Packaging evidence and remaining acceptance

The [packaging checkpoint](preview-packaging-readonly-handoff.md) distinguishes the earlier read-only snapshot from the later parent-confirmed packaging commit/push. Application and packaging branches remain separate; no packaging-owned config or script was edited by this application slice.

Complete-profile cold backup/restore is the next slice and remains unimplemented. It must stop all owners, inventory credentials and persistent files, preserve the shared store, restore into an isolated root, explicitly rebase approved paths, invalidate copied runtime descriptors, and verify the same UUIDs, exact history, presentation and decryption. Existing per-Entity restore creates a new UUID and does not satisfy this requirement.

Fake credential adapters do not establish Windows Credential Manager or DPAPI integration. Ownership headers are non-secret routing guards, not local authentication or binary provenance. Actual daemon/GUI lifecycle, signed-installer coexistence, ongoing-Entity upgrade/reopen, cold restore and all ten exact-installer stages remain separately required. None is accepted by these component results.

Stable/main/beta promotion, release tags, public preview selection, installer publication and production use remain outside this component validation.
