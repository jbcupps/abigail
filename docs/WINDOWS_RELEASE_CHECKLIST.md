# Windows release checklist

Use this with [the release runbook](RELEASE_RUNBOOK.md) and [SSL.com cloud signing](windows-cloud-signing.md). Signing credentials and installer acceptance belong to the existing release lane; the final-artifact and public-download checks only inspect files and never sign or launch them.

## Choose the revision and channel

- Record the full source commit, intended version, channel, signing mode, and expected publisher certificate thumbprint. Review the product diff before promotion: a beta-based packaging branch can contain product changes that are absent from main.
- Require a passing `CI / gate` for the exact publishing commit on `beta` or `main`. A successful older commit or a different branch head is insufficient. Advisory jobs do not replace this gate.
- Check that the version is unused. The workflow refuses to replace a release or reuse a tag pointing at different source. Never overwrite installer bytes under an existing release name.
- Use `publish_release=false` for a candidate. Its provenance says `candidate`; the workflow creates no release or tag and the publication validator rejects candidate manifests. A signed candidate still invokes the signing service and existing installed acceptance tests, so obtain any required operational approval before dispatch.
- `beta` publishes a `vX.Y.Z-beta.N` prerelease and leaves latest stable unchanged. `stable` publishes a signed `vX.Y.Z` release and explicitly makes it latest. Unsigned beta stabilization is allowed and must say `windowsSigned: false`; unsigned stable publication is rejected.

Example candidate dispatch, after approval and selection of an unused version:

```powershell
gh workflow run release.yml --repo jbcupps/abigail --ref <reviewed-branch> -f release_version=<unused-version> -f windows_signing=esigner -F publish_release=false
```

Record the resulting run's full `headSha` and verify it equals the reviewed commit. A branch name can move. Candidate success is evidence for that commit and channel; promotion or any later code change requires fresh validation. Do not relabel or edit candidate provenance to publish it.

## Verify the bytes that will be published

The workflow requires these steps in order:

1. Build, sign when requested, and run the existing installed acceptance checks.
2. `stage_release_assets.ps1` selects exactly one NSIS installer and at most one applicable MSI, copies them to canonical public filenames, and rejects stale output and duplicate names. Cloud signing publishes NSIS only. Optional updater files and their generated manifest are staged before hashing.
3. `verify_release_artifacts.ps1` uses Windows Authenticode and SignTool verification against the configured certificate thumbprint. Signed EXEs require a valid chain, Code Signing usage, SHA-256 digest, and trusted SHA-256 RFC 3161 timestamp. The local MSI path requires valid publisher and timestamp verification. An unsigned policy accepts only actually unsigned files, not broken signatures.
4. `release_provenance.mjs create` records the exact commit, tag, channel, signing policy, size, and SHA-256 for every staged asset except the provenance file itself. Its signature evidence must cover every installer and match those hashes. This manifest is a consistency record; it is not a separate cryptographic attestation.
5. Upload one `abigail-release-assets-windows` Actions artifact. The publishing job downloads this artifact and repeats hash, policy, and native signature checks before creating a tag or release. No file is renamed or rebuilt after these checks.

Review `windows-signatures.json` for the existing installed payload checks, `windows-signature-evidence.json` for the canonical final installer, and `release-provenance.json` for the exact file set. Tauri updater `.sig` files are independent of Windows Authenticode.

The required repository configuration for the existing cloud lane is documented in the cloud guide. Verify presence of required secrets by name only; never copy their values into reports. A successful previous run establishes prior service access, not current entitlement, billing status, or future availability. Do not create credentials or change certificate trust to make a check pass.

## Verify public distribution

The `verify-published-download` job must succeed before announcing release completion. Its verifier checks:

- Release tag, draft/prerelease state, exact `target_commitish`, and remote tag resolution.
- The exact published asset set, uploaded state, size, and GitHub SHA-256 digests, including the provenance file's own bytes.
- Fresh downloads through public tagged asset URLs against the original build artifact, followed by native Windows signature verification.
- For stable, latest-release metadata and the website's actual `/releases/latest/download/Abigail-windows-x64-setup.exe` URL. For beta, explicit tagged downloads and preservation of a stable latest release.

Keep the `published-download-verification` Actions artifact. `result.json` must say `verified: true` and record the expected tag, commit, URL, and installer hash. Merely completing the upload step is insufficient. Beta also retains the existing `verify-beta-installer` payload inspection.

For a reported warning, request the exact downloaded filename, SHA-256, release URL, and warning text or screenshot. A missing or invalid signature is a packaging/trust problem. A valid signature can still receive SmartScreen reputation warnings; passing this checklist does not promise a warning-free launch. See [Microsoft SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation) and [SignTool verification](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool).

## Failure and recovery

- **Failure before publication:** no release should be created. Preserve the run evidence, fix the cause on a development branch, and rebuild. If a tag was created immediately before an upload failure, inspect both tag and release state before retrying.
- **Failure after publication:** treat distribution as unverified even if users can download it. Preserve the original Actions artifact, public metadata, hashes, and failed `result.json`. Do not overwrite assets, change trust settings, or silently declare success.
- **Transient public-read failure:** after confirming no asset or release metadata changed, rerun only the failed verification job with its original build artifact. The verifier uses a fresh download directory and does not modify a release.
- **Wrong bytes, publisher, tag, or channel:** stop promotion and announcements. The release owner must explicitly approve any quarantine, latest-channel correction, or removal. Ship a corrected build under a new unused version after the normal gate and signing checks; do not rewrite the old version's provenance.
- **Missing signing credentials or service entitlement:** resolve through the existing account owner and provider. Approval is required for new enrollment, purchases, key provisioning, and account changes. Unsigned candidates remain available for diagnostics.

### Pinned v0.0.76 publication recovery

`recover-stable-0.0.76.yml` is a one-time exception to rebuilding after a prepublication failure. Original release run `39380331517` completed its build, signing, installed acceptance, and asset verification successfully. Publication stopped when its expected missing-release response retained a native exit code of 1. No tag or release creation step ran.

The recovery accepts no inputs. It pins the original stable source `e3ad51181da1ea11bded24ca0c39e2dcfe5fa944`, artifact `12246127943`, archive digest, independently verified provenance hash, and successful main-push CI gate `39376395634`. It checks original build/acceptance step results, verifies the exact four files and publisher again, and publishes without rebuilding, signing, or editing provenance. It refuses an existing release or a latest stable release other than `v0.0.75`. The recovery workflow revision is recorded separately and is never substituted for the product source.

Run this workflow only from reviewed `main`. Completion still requires its public-download verifier to succeed and `published-verification/result.json` inside the `stable-0.0.76-recovery-verification` artifact to report `verified: true`. Retain both the original failed publication run and the new recovery evidence; the old workflow is not relabeled successful.

## Local process tests

Run on Windows with Node.js, PowerShell 7, and the Windows SDK SignTool available:

```powershell
node scripts/check_unsigned_stabilization.mjs
node --test scripts/tests/test_release_provenance.mjs
pwsh ./scripts/tests/test_release_artifacts.ps1
pwsh ./scripts/tests/test_stage_release_assets.ps1
pwsh ./scripts/tests/test_published_release.ps1
pwsh ./scripts/tests/test_stable_recovery.ps1
```

The tests exercise missing/tampered assets, identity and channel mismatches, wrong publisher, missing timestamps, ambiguous staging, and substituted public downloads. Native signature tests copy an already signed Node executable and never sign or execute the test copy. Published-download tests mock network metadata and transfers while retaining the real hash and native signature checks. Junction tests need filesystem permissions supported by the Windows CI runner. These tests do not prove signing-service availability or substitute for a hosted candidate and approved release run.
