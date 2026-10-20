# Combined preview development validation

2026-10-20, Desktop-Newused. Development branch:
`dev/preview-combined-0.0.76`.

## Selected source

| Input | Commit |
| --- | --- |
| Application, `dev/first-signal-integration-0.0.76` | `ca6a7527da8787be6b5315e13e4fc228287ce15d` |
| Packaging, `dev/preview-packaging-0.0.76` | `de65e7e47fa35c7b742421274db5e1fac4720f8d` |
| Included release scaffold | `6b246d3ab07322a899106d1198bfafba3a7b2309` |
| Shared main base | `17b636b9e5f10b812deb287ad2c6637188991ec2` |
| Separate beta comparison source | `ea92effd31f554f74bb5e109deaa42876ec4f06f` |

Remote heads were read before cloning published source into a new isolated
checkout. No combined remote branch existed. The application and packaging
commits are merged directly; beta is not merged. The README conflict was
resolved by retaining First Signal development details and the preview
preservation/acceptance rules.

Both overlays now describe the implemented runtime interface, with explicit
`candidateAcceptance: "pending"`. The validator checks the complete closed
launch schema, types and nested fields. Static roots, UUIDs, endpoints and
executable recipes are rejected. The one application correction restricts
Entity handshake exemptions to GET `/health`, GET `/v1/status` and OPTIONS,
matching Hive and the documented contract. Stable header rejection is unchanged.

The stable base Tauri configurations are identical to the application input.
The staging, signed-builder, signing callback, release workflow and unavailable
preview selector are identical to the selected packaging input. Full cold
snapshot/restore remains a separate application task.

## Current-source checks

Counts below are distinct final checks, not totals accumulated across retries.
The prior application's 167 selected Rust passes and prior packaging evidence
remain historical; they are not relabeled as combined acceptance.

| Check | Result |
| --- | --- |
| Hive UI, `npm test` | [281 tests in 11 files passed](../output/preview-combined/evidence/hive-frontend-tests-permitted.txt) |
| Runtime UI, `npm test` | [92 tests in 7 files passed](../output/preview-combined/evidence/runtime-frontend-tests.txt) |
| Both UI production builds, `npm run build` | TypeScript and Vite passed: [Hive](../output/preview-combined/evidence/hive-frontend-build.txt), [Runtime](../output/preview-combined/evidence/runtime-frontend-build.txt) |
| `test_preview_packaging.ps1` | [70 policy cases passed](../output/preview-combined/evidence/review-policy.txt) |
| `test_preview_staging.ps1` | [21 mocked staging checks passed](../output/preview-combined/evidence/review-staging.txt) |
| `test_signed_installer_state.ps1` | [38 prepared-state assertions passed](../output/preview-combined/evidence/review-prepared-state.txt) |
| `test_preview_signed_capture.ps1` | [23 synthetic capture cases passed](../output/preview-combined/evidence/launcher-capture-tests.txt) |
| `test_preview_nsis_hooks.ps1` | [32 checks passed](../output/preview-combined/evidence/nsis-hook-compile-tests-permitted.txt), including five compilation fixtures |
| Node release provenance and preview channel suites | [77 tests passed, zero skipped](../output/preview-combined/evidence/release-node-tests-permitted.txt) |
| `test_prepare_preview_assets.ps1` | [22 mocked native-verification/policy checks passed, zero skipped](../output/preview-combined/evidence/asset-policy-tests-permitted.txt) |
| `check_unsigned_stabilization.mjs` | [Passed](../output/preview-combined/evidence/unsigned-guard-check.txt) |
| Changed Rust formatting and Git whitespace | Passed |
| Independent runtime and combined-diff reviews | Completed; no outstanding scoped findings |
| Entity `preview_binding_tests`, compiled preview/release | [4 tests passed](../output/preview-combined/evidence/entity-preview-binding-tests.txt); other integration cases were filtered out |
| All four release preview binaries and staging | [Built and staged successfully](../output/preview-combined/evidence/preview-staging-build.txt) with `ABIGAIL_BUILD_PROFILE=preview` |
| Final unsigned Prepare source/overlay/contract/binary hashes | Post-commit receipt is recorded separately in local `build-state.json` and the task handoff; it cannot be embedded in its own source snapshot |

The first Hive frontend attempt failed before test collection because sandbox
filesystem restrictions prevented Vite's temporary-config rename. The permitted
retry passed all 281 tests. Initial release/asset-policy runs skipped junction
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
Existing unused-helper warnings were emitted; no build failure occurred.
No compiler optimization setting or persistent Rust flag was changed.

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
- Integrate the separately developed full cold-profile snapshot/restore and
  verify shared-store state, identity, presentation and credential availability.
- Validate real isolated Windows Credential Manager/DPAPI behavior and schema
  refusal with byte preservation, then return to unchanged stable.
- Deliberately compare beta Genesis/static-runtime/installer-lock behavior with
  this main-based source before promotion; no blind beta merge.

Component checks and rebuilt artifacts cannot inherit signed-installer
acceptance from earlier work. Later signing/publication must preserve the
accepted exact installer bytes and all existing release guards.
