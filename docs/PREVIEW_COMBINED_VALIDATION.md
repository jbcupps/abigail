# Combined preview development validation

2026-10-20, Desktop-Newused. Development branch:
`dev/preview-combined-0.0.76`.

## Selected source

| Input | Commit |
| --- | --- |
| Application, `dev/first-signal-integration-0.0.76` | `ca6a7527da8787be6b5315e13e4fc228287ce15d` |
| Published cold-recovery descendant on that application branch | `4558fe68d9812998eb53a94ba157970af552f923` |
| Packaging, `dev/preview-packaging-0.0.76` | `de65e7e47fa35c7b742421274db5e1fac4720f8d` |
| Included release scaffold | `6b246d3ab07322a899106d1198bfafba3a7b2309` |
| Shared main base | `17b636b9e5f10b812deb287ad2c6637188991ec2` |
| Separate beta comparison source | `ea92effd31f554f74bb5e109deaa42876ec4f06f` |

Remote heads were read before cloning published source into a new isolated
checkout. No combined remote branch existed. The application and packaging
commits are merged directly; beta is not merged. The README conflict was
resolved by retaining First Signal development details and the preview
preservation/acceptance rules.
After the inline-style correction was committed and prepared, the parent
authorized integration of the published cold-recovery descendant. Its full SHA
was resolved from the remote before fetching it. Recovery code merged cleanly;
five documentation conflicts were reconciled with the combined contract and
acceptance boundary.

Both overlays now describe the implemented runtime interface, with explicit
`candidateAcceptance: "pending"`. The validator checks the complete closed
launch schema, types and nested fields. Static roots, UUIDs, endpoints and
executable recipes are rejected. The routing correction restricts
Entity handshake exemptions to GET `/health`, GET `/v1/status` and OPTIONS,
matching Hive and the documented contract. Stable header rejection is unchanged.
The frontend correction embeds the unchanged shared Presence stylesheet through
each shell's existing inline stylesheet pipeline. Production preparation exposed
the original side-effect CSS import as incompatible with the packaged WebView;
the packaging guard remains unchanged.

The stable base Tauri configurations are identical to the application input.
The staging, signed-builder, signing callback, release workflow and unavailable
preview selector are identical to the selected packaging input. The separately
developed cold snapshot/verify/restore source is now included. All four preview
processes retain shared lifetime leases; cold operations require exclusive
profile-use and Hive ownership. Normal startup refuses partial staging roots.
The offline `abigail-profile` utility is built and tested separately; it is not
one of the four installer payloads. Its exact installed delivery and recovery
workflow remain acceptance work.

## Current-source checks

Counts below are distinct final checks, not totals accumulated across retries.
The prior application's 167 selected Rust passes, imported cold-recovery
104-test/seven-CLI evidence and prior packaging evidence remain historical;
only explicitly listed reruns below are new combined checks. They are not
relabeled as installed acceptance.

| Check | Result |
| --- | --- |
| Hive UI, `npm test` | [283 tests in 11 files passed](../output/preview-combined/evidence/css-fix-hive-tests.txt) |
| Runtime UI, `npm test` | [94 tests in 7 files passed](../output/preview-combined/evidence/css-fix-runtime-tests.txt) |
| Both UI production builds, `npm run build` | TypeScript and Vite passed: [Hive](../output/preview-combined/evidence/css-fix-hive-build.txt), [Runtime](../output/preview-combined/evidence/css-fix-runtime-build.txt) |
| `test_preview_frontend_assets.ps1` | [6 actual generated-artifact checks passed](../output/preview-combined/evidence/css-fix-frontend-assets.txt), exercising the unchanged preparation guard, no emitted CSS files, and embedded Presence styles |
| `test_preview_packaging.ps1`, after recovery integration | [76 policy cases passed](../output/preview-combined/evidence/recovery-packaging-policy.txt) |
| `test_preview_staging.ps1`, after recovery integration | [21 mocked staging checks passed](../output/preview-combined/evidence/recovery-staging-tests.txt) |
| `test_signed_installer_state.ps1`, after recovery integration | [38 prepared-state assertions passed](../output/preview-combined/evidence/recovery-prepared-state-tests.txt) |
| `test_preview_signed_capture.ps1`, after recovery integration | [23 synthetic capture cases passed](../output/preview-combined/evidence/recovery-launcher-capture-tests.txt) |
| `test_preview_nsis_hooks.ps1` | [32 checks passed](../output/preview-combined/evidence/nsis-hook-compile-tests-permitted.txt), including five compilation fixtures |
| Node release provenance and preview channel suites | [77 tests passed, zero skipped](../output/preview-combined/evidence/release-node-tests-permitted.txt) |
| `test_prepare_preview_assets.ps1` | [22 mocked native-verification/policy checks passed, zero skipped](../output/preview-combined/evidence/asset-policy-tests-permitted.txt) |
| `check_unsigned_stabilization.mjs` | [Passed](../output/preview-combined/evidence/unsigned-guard-check.txt) |
| Changed Rust formatting and Git whitespace | Passed |
| Independent runtime and combined-diff reviews | Completed; no outstanding scoped findings |
| Recovery snapshot/state, compiled preview/release | [29 passed under the original long TEMP after the fixture correction](../output/preview-combined/evidence/recovery-snapshot-longpath-final.txt) |
| Recovery process leases, compiled preview/release | [9 passed, one ignored child fixture](../output/preview-combined/evidence/recovery-lease.txt) |
| Existing-credential recovery with fake probes, compiled preview/release | [7 passed](../output/preview-combined/evidence/recovery-credentials.txt) |
| Profile regression, compiled preview/release | [14 passed, one ignored child fixture](../output/preview-combined/evidence/recovery-profile.txt) |
| Fake credential isolation, compiled preview/release | [5 passed](../output/preview-combined/evidence/recovery-preview-credentials.txt) |
| Config/global-config regression, separate default-mode release test target | [37 passed](../output/preview-combined/evidence/recovery-config-default.txt), with named preview variables verified absent |
| Actual SurrealKV cold recovery, compiled preview/release | [3 passed, one ignored child fixture](../output/preview-combined/evidence/recovery-store.txt); 15 fresh-process phase records |
| Entity `preview_binding_tests`, compiled preview/release after recovery integration | [4 tests passed](../output/preview-combined/evidence/recovery-binding.txt); other integration cases were filtered out |
| Separate offline recovery CLI | [7 help/parser/missing-bundle refusal checks passed](../output/preview-combined/evidence/recovery-cli-smoke.txt); no successful system-credential restore executed |
| All four release preview binaries and staging before the CSS correction | [Built and staged successfully](../output/preview-combined/evidence/preview-staging-build.txt) with `ABIGAIL_BUILD_PROFILE=preview`; final preparation rebuilds these after the correction |
| Final unsigned Prepare source/overlay/contract/binary hashes | The post-commit result and receipt are recorded separately in local `build-state.json` and the task handoff; they cannot be embedded in their own source snapshot |

The final selected Rust matrix is **108 distinct passing tests**: 71 compiled
as preview and 37 default-mode config tests. Three ignored subprocess helpers
are exercised by their parent fixtures and are not extra passes. The seven CLI
checks and 377 frontend tests are separate counts. Recovery changed no frontend
source after the recorded CSS-correction suites. The repeated snapshot run
replaces its earlier short-TEMP run rather than adding another 29 passes.
The [preview execution summary](../output/preview-combined/evidence/recovery-validation-summary.txt),
[default-mode build/source proof](../output/preview-combined/evidence/recovery-config-default-proof.txt)
and [final source fingerprint](../output/preview-combined/evidence/recovery-final-source-files.txt)
retain the exact commands and sources. Only the reviewed test fixture changed
after the first successful preview selections; the snapshot selection and CLI
were rebuilt/rechecked afterward.

Initial frontend attempts failed before test collection because sandbox
filesystem restrictions prevented Vite's temporary-config rename. Permitted
retries passed; the final 377-test run includes four new inline Presence style
regressions and supersedes the earlier 373-test run. Initial release/asset-policy runs skipped junction
fixtures under the restricted filesystem; permitted retries ran all cases.
NSIS was found in the existing Tauri tool cache; after an initial restricted
process-launch failure, its permitted fixture compilation passed. No generated
fixture installer was executed. These superseded attempts add no passing count.

The NSIS, launcher capture, asset-policy and prepared-state tests use synthetic
inputs. Native signature verification in the asset-policy fixture is mocked.
They do not establish signing, actual installer behavior or coexistence.
Linked logs retain the actual test output with ANSI sequences removed and local
repository/user/temporary paths replaced by placeholders. Expected negative-case
diagnostics are preserved. Unsanitized build logs and superseded attempts remain
local; repeated policy runs are not counted again.

Recovery reruns use `--locked --offline --release` with synthetic workspace
fixtures and serial test execution. The legacy config selection runs in a
separate default-mode test target: compiling those tests as preview initially
produced 19 passes and 18 failures from required preview preflight because their
fixtures do not initialize preview context. An initial default-target runner
also left `ABIGAIL_PROFILE` present but empty when clearing environment values;
that correctly triggered the same refusal. A child-process presence check
identified the runner issue, and the runner now removes those variables and
verifies their absence before invoking Cargo. Both failed attempts are retained
separately and excluded from final passing totals. Production preflight was not
weakened.
The remaining recovery and routing selections use compiled preview mode.
Only the named fake-credential filters run; broader vault tests that invoke
real Windows DPAPI are excluded.
The [mis-scoped preview config run](../output/preview-combined/evidence/recovery-config.txt),
[default runner failure](../output/preview-combined/evidence/recovery-config-default-attempt1-empty-env.txt)
and [environment-cleanup proof](../output/preview-combined/evidence/recovery-config-environment-cleanup-proof.txt)
preserve those diagnostics.

The first snapshot selection produced 28 passes and one internal state-fixture
failure when an unprefixed atomic temporary filename reached 289 characters.
All 16 public snapshot/restore tests passed in that same long-TEMP run. Review
confirmed that public restore preserves Windows extended canonical paths
through staging and rebasing; the direct unit fixture bypassed that precondition.
The unchanged snapshot selection then passed 29 tests with shorter TEMP. The
test fixture now canonicalizes its created container before deriving paths,
matching public restore; no production atomic-write behavior changed. This is
preserved as test-fixture evidence, not a reproduced public restore failure or
an approval denial. The corrected fixture passed all 29 snapshot/state tests
under the original long TEMP.
The [original long-path failure](../output/preview-combined/evidence/recovery-snapshot-attempt1-longpath.txt)
and [successful final long-path proof](../output/preview-combined/evidence/recovery-snapshot-longpath-final-proof.txt)
remain separate.

## Native build and preparation

The candidate uses Rust/Cargo 1.97.0, Node 24.13.1, npm 11.8.0 and Tauri CLI
2.11.4. Because the system drive lacked room for a new Rust build, native outputs
use a new candidate-specific target on E:, separate from every prior worker's
checkout and build directory.

The successful staging invocation was:

```powershell
$env:CARGO_TARGET_DIR = '<isolated-cache>/preview'
./scripts/stage_split_installer_resources.ps1 -Configuration release -Channel preview -Version 0.0.76
```

Staging builds both daemons with no Tauri overlay, then each shell with its own
overlay, always setting `ABIGAIL_BUILD_PROFILE=preview`. It checks numeric
version, embedded current frontend entries, preview identifiers and contract
markers before copying the three internal resources.
The initial daemon build completed in 13m36s; staging compiled Hive in 2m47s
and Runtime in 54.69s. The filtered Entity test harness compiled in 22.19s.
Existing unused-helper warnings were emitted; native compilation succeeded.
No compiler optimization setting or persistent Rust flag was changed.

The first post-merge Prepare, against
`1643b5a5f6b875ebfc1b7b8562b568cc34c1c000`, stopped at
`Read-FrontendAssets`: both frontends emitted an external Presence stylesheet.
No frozen build-state was produced. This was an executed validation failure,
not an approval denial. Moving the shared stylesheet into both existing inline
CSS pipelines fixes that incompatibility without changing its contents or
weakening the guard. The final artifact test also verifies layout, animation and
reduced-motion markers in both generated JavaScript entries. This correction
requires new native shell builds and new preparation hashes.

The second preparation completed successfully at
`3875299954976fc37785a48e924fd89d86d51113`, before recovery integration.
It rebuilt all four preview binaries, passed the unchanged frontend guard,
verified all signatures as `NotSigned` and passed the pure prepared-state
validator with identical compiled/staged/prepared bytes. Its source-specific
receipt and state/config copies are retained locally as intermediate evidence.
The recovery descendant and new lifecycle metadata supersede those hashes;
they require a fresh preparation after the final integration commit.

Final `build_signed_installer.ps1 -Phase Prepare -Channel preview -Version
0.0.76 -TargetDir <isolated-cache>` runs after the source commit. It freezes that
commit and clean source snapshot, exact overlay/contract hashes, bundle config
and all four unsigned executable hashes. The resulting local `build-state.json`
is kept outside tracked source to avoid invalidating its own snapshot.
Preparation alone does not sign, bundle or execute an installer. Its local
receipt must be checked against the final committed source and exact current
bytes; the pre-commit staging results above do not stand in for that receipt.

## Acceptance and publication boundary

No normal GUI/daemon startup, installer execution, signing, real credential use,
release/tag/Pages publication, main/beta update or new access grant is part of
this integration. Existing stable 0.0.76 remains the available release. The
preview selector stays unavailable.

All ten [exact-installer stages](PREVIEW_CHANNEL.md#required-acceptance-evidence)
remain open. In particular:

- Run actual combined Hive/Runtime/daemon lifecycle, both launch orders, port
  races, stale identities/endpoints, and verified close/restart.
- Test stable/preview install, upgrade and uninstall coexistence in disposable
  Windows fixtures against exact candidate bytes.
- Verify ongoing Entity UUID/history/settings preservation across upgrade and
  reopening, with a recorded first-preview baseline.
- Validate the integrated full cold-profile snapshot/restore with the exact
  installed candidate, preserving shared-store state, identity, presentation and
  credential availability. Its synthetic fresh-process proof does not establish
  normal GUI reopening or WebView-store compatibility.
- Validate real isolated Windows Credential Manager/DPAPI behavior and schema
  refusal with byte preservation, then return to unchanged stable.
- Deliberately compare beta Genesis/static-runtime/installer-lock behavior with
  this main-based source before promotion; no blind beta merge.

Component checks and rebuilt artifacts cannot inherit signed-installer
acceptance from earlier work. Later signing/publication must preserve the
accepted exact installer bytes and all existing release guards.
