# Hosted preview harness preparation evidence

2026-10-20, isolated `dev/preview-combined-0.0.76` checkout on Newused.
This records **code review and credential-free harness checks**, followed by
the first hosted attempt and its preparation repair below. No completed hosted
acceptance is claimed. The approval scope is in
[PREVIEW_HOSTED_ACCEPTANCE.md](PREVIEW_HOSTED_ACCEPTANCE.md).

## Immutable product inputs

- Combined product: `f7c7848d25726480b253ec1ca637c3ffa5ea3c77`.
- Application input: `ca6a7527da8787be6b5315e13e4fc228287ce15d`.
- Application cold-recovery descendant: `4558fe68d9812998eb53a94ba157970af552f923`.
- Packaging input: `de65e7e47fa35c7b742421274db5e1fac4720f8d`.
- Packaging release scaffold: `6b246d3ab07322a899106d1198bfafba3a7b2309`.
- Actual published stable comparison: `v0.0.76`,
  `e3ad51181da1ea11bded24ca0c39e2dcfe5fa944`.

The harness change edits only development workflow, acceptance tooling and
documentation. Application crates, both app roots, existing release workflows
and signing implementations remain unchanged from the combined product commit.
The future workflow checks out the product and reviewed harness separately.
It does not reuse or relabel locally prepared executable bytes.

Stable release metadata was read through GitHub's API. Release ID `418137720`,
the four asset IDs/sizes/SHA-256 values, public signer thumbprint and installed
payload hashes are recorded in
[`preview-candidate-baseline.json`](../scripts/preview-candidate-baseline.json).
The future workflow verifies the downloaded bytes and native signature before
signing a candidate.

## Fresh local checks

| Command | Result |
| --- | --- |
| `pwsh -NoProfile -File scripts/tests/test_preview_candidate_workflow.ps1` | 41 policy, scope-guard and inline PowerShell parser checks passed |
| `pwsh -NoProfile -File scripts/tests/test_preview_candidate_runner.ps1` | 111 pure/injected installer-policy, sequencing, evidence, WebView2 prerequisite and default-plan checks passed |
| `pwsh -NoProfile -File scripts/tests/test_preview_owned_job.ps1` | 66 pure/mock/parser/C# compilation checks passed; no native Job Object API invoked |
| `node --test scripts/tests/test_preview_candidate_lifecycle.mjs` | 38 tests passed, zero failed or skipped |
| `node --check` for the contract and lifecycle modules | Passed |
| `node scripts/tests/preview-candidate-lifecycle.mjs --plan` | Inert plan returned; no product started |
| Full workflow YAML parse using PyYAML BaseLoader | Passed; manual-only trigger and `contents: read` verified |
| Git whitespace and product-source boundary checks | Passed |

Tests use synthetic values, injected process/registry/signature boundaries and
text files. They do not execute application binaries or installers, inspect
local credentials, call DPAPI, sign, query SSL.com quota, or dispatch workflows.
C# compilation loads declarations and exercises pure quoting/environment
functions, not native process creation.

## Review findings resolved

- Accept uppercase SHA-256 strings emitted by the unchanged product signer,
  while verifying exact bytes.
- Align workflow, PowerShell and Node paths, including copying and rechecking
  the recovery CLI inside the owned fixture before use.
- Supply the stable helper's required hosted repository metadata.
- Require verified preview identity before shutdown; prevent duplicate stable
  control writes and release uncertain Node handles without claiming process
  quiescence or killing a discovered PID.
- Validate restored profile namespace and original master public key as well as
  entity IDs, receipts, history and presentation.
- Preserve only reviewed lifecycle assertion names/booleans and failure points
  in public evidence; retain raw state/logs privately.
- Require an existing WebView2 registration before build/signing and installation.
  Detection follows official Tauri CLI `2.11.4` NSIS template blob
  `d372e3c391770cf231db974422a1e4f8adaac3a6`. Missing prerequisite stops the run;
  no bootstrap/update or warning bypass is authorized.

Independent review covered workflow/installer output compatibility and private
Windows Job Object ownership, plus runtime API/cold-recovery contracts. Final
checks above were rerun after integration fixes.

## Not executed / remaining boundaries

No hosted build, normal product launch, real DPAPI fixture, installation,
signing or dispatch occurred during this preparation. No production/main/beta,
release tag, preview selector or repository access change was made.

A new manual workflow needs separately authorized default-branch registration.
One dispatch then requires action-specific approval for the disposable stable
and preview identities/DPAPI material, exact installer lifecycle and owned
process teardown, and six expected SSL.com signing operations using existing
references. Available signing quota, marginal charge and Actions allowance
remain unknown. No purchase or upgrade is implemented.

Even a green future run does not establish native GUI/WebView reopening,
KnownFolder bootstrap, native Credential Manager support, stable-active
installer behavior, a prior-preview migration, cross-user/device recovery,
newer-schema refusal, or all ten publication acceptance stages. Reports retain
`publicationEligible: false` and `fullAcceptance: false`.

## First hosted attempt and dependency bootstrap repair

After separately approved workflow registration through
[PR #298](https://github.com/jbcupps/abigail/pull/298), run
[`39467855613`](https://github.com/jbcupps/abigail/actions/runs/39467855613)
executed harness `77f08ae6abb228b30df153c65cd1646557d6fb08`, attempt 1,
against the unchanged product and stable pins above.

Hosted scope checks, harness tests, existing WebView2 prerequisite detection,
pinned stable downloads/signature verification, and tool setup passed.
Preparation failed when Hive compiled imported Runtime source before Runtime's
dependencies existed: TypeScript could not resolve `@tauri-apps/api/core`.
Signing, fixture installation/lifecycle and artifact upload were skipped.
This run made no SSL.com signing operations and created no fixture identities.

The workflow now installs Runtime's locked dependency tree before invoking the
unchanged product preparation script. Product source, dependency lockfiles,
signing implementation, approval guards and fixture scope remain unchanged.

A fresh extraction of both frontends from product
`f7c7848d25726480b253ec1ca637c3ffa5ea3c77`, using Node 24.13.1 and npm 11.8.0,
reproduced the exact missing-module failure with only Hive dependencies
installed. Installing Runtime dependencies with `npm ci` then made both
`tsc && vite build` commands pass. No application or installer was executed.
Independent review found no blocking repair defects.

The repaired workflow passes **45** credential-free policy, scope and inline
PowerShell parser checks, including removal, unlocked-install, ignored-failure
and late-install mutations for the new bootstrap. No audit remediation was
performed. Native builds, signing and installed lifecycle acceptance remain
pending a subsequent reviewed dispatch.
