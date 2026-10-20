# Hosted preview candidate acceptance

2026-10-20. This is the review and approval scope for a separate, nonpublishing
manual Windows runner. Current authorization covers preparing, reviewing,
testing and pushing its development source. **No hosted dispatch, signing,
installer execution or fixture credential/DPAPI operation is authorized yet.**
The stable release workflow, main/beta branches, release tags, Pages and
`docs/preview/current.json` remain unchanged.
Fresh local checks and review fixes are recorded in
[PREVIEW_HOSTED_HARNESS_VALIDATION.md](PREVIEW_HOSTED_HARNESS_VALIDATION.md).

## Exact sources and execution boundary

| Input | Required identity |
| --- | --- |
| Repository and workflow | `jbcupps/abigail`, **Abigail Preview Candidate**, [`.github/workflows/preview-candidate.yml`](../.github/workflows/preview-candidate.yml) |
| Product source | `f7c7848d25726480b253ec1ca637c3ffa5ea3c77`, checked out separately from the harness |
| Harness source | The full reviewed commit supplied as `reviewed_harness_commit`, matching the dispatch revision |
| Stable comparison | Published `v0.0.76`, actual source `e3ad51181da1ea11bded24ca0c39e2dcfe5fa944` |
| Stable bytes and publisher | Exact release asset IDs, sizes, SHA-256 values, installed payload hashes and certificate thumbprint in [`preview-candidate-baseline.json`](../scripts/preview-candidate-baseline.json) |

The harness uses `windows-latest` on a fresh GitHub-hosted Windows machine and
`contents: read`.
It does not run on the operator's desktop or a self-hosted runner. Product
checkout (`product/`) and exact candidate bytes are distinct from the harness
checkout (`harness/`).
A fresh hosted build produces new hashes; local preparation and older test
results cannot be relabeled as acceptance of those bytes. The offline recovery
CLI is built from the same pinned product source but remains separate from the
four installer payloads.

The PowerShell entry point is [`run-preview-candidate.ps1`](../scripts/run-preview-candidate.ps1);
its default behavior without `-Execute` is a plan. Installer and lifecycle execution belong only to the
explicit hosted run path. [`preview-candidate-lifecycle.mjs`](../scripts/tests/preview-candidate-lifecycle.mjs)
owns the synthetic `seed`, `verify` and `stable-return` phases.
[`preview-owned-job.ps1`](../scripts/tests/preview-owned-job.ps1) confines child
processes to the exact fixture job.

## Approval needed before dispatch

Approval must name the workflow above, its full reviewed harness commit, the
pinned product source and one hosted run. The manual inputs are:

| Input | Default / scope |
| --- | --- |
| `reviewed_harness_commit` | Required lowercase 40-character reviewed harness SHA equal to the dispatched `github.sha`, on a `dev/*` branch; it is not a product-source override or an approval by itself |
| `approve_fixture_dpapi` | `false`; approval must cover fresh **preview and stable** fixture identities and the same hosted user's DPAPI material, plus isolated install/uninstall and owned-process teardown described below |
| `approve_signing_quota` | `false`; approval must cover one candidate signing pass, expected to consume about six signing requests: four payloads, NSIS uninstaller and final installer |

The booleans record an action-specific human approval; setting them does not
create permission. Existing SSL.com secret references are available only to the
signing step. No new certificate, account, service enrollment, access grant,
provider key or model subscription is requested. The installer runner does not
sign. An uncertain signing outcome must be checked before another request;
approval for one pass is not approval for repeated quota-consuming reruns.
The workflow refuses `GITHUB_RUN_ATTEMPT` values other than `1`; a repeat needs
a newly reviewed dispatch. It shares the existing signing concurrency group and
does not cancel an active signer. Available quota or its monetary cost has not
been established; the harness cannot purchase quota or upgrade the account.
It uses the existing `ESIGNER_USERNAME`, `ESIGNER_PASSWORD`,
`ESIGNER_CREDENTIAL_ID`, `ESIGNER_TOTP_SECRET` and
`WINDOWS_CERTIFICATE_THUMBPRINT` references. The unchanged signing-tool setup script downloads
official SSL.com CodeSignTool 1.3.3 and requires archive SHA-256
`317D429BE3AA12A5F2C1FFDD575EAB0CB0CE5E2408AB0056BCDCAAB29875F73D`.
No signing requests or quota inquiries were made during harness preparation.
GitHub-hosted Windows compute may also consume the repository's Actions
allowance; available allowance and any marginal charge are not established.

GitHub requires a new `workflow_dispatch` workflow to exist on the default
branch before it can be triggered; see [GitHub's trigger documentation](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch).
A dev push does not authorize or perform that registration. If the workflow is
not registered, stop and request the separate reviewed default-branch action.
Do not modify the release workflow, add another trigger, grant permissions or
use a dispatch workaround.

After registration and action-specific approval, the selected command is:

```powershell
gh workflow run preview-candidate.yml --repo jbcupps/abigail `
  --ref dev/preview-combined-0.0.76 `
  -f reviewed_harness_commit=<full-reviewed-harness-commit> `
  -f approve_fixture_dpapi=true -f approve_signing_quota=true
```

This is a proposed command, not an executed dispatch. Confirm the development
branch still equals the reviewed harness commit immediately before using it.

## Fixture data and credential scope

Each run creates a new root beneath `RUNNER_TEMP`:

```text
abigail-preview-<GITHUB_RUN_ID>-<GITHUB_RUN_ATTEMPT>-<generated GUID>/
  install-preview/
  install-stable/
  profiles/preview/
  profiles/stable/
  profiles/restored-preview/
  snapshots/preview/
  control/lifecycle-state.json
```

These are runtime path components, not fixed profile IDs or release metadata.
The harness creates a new preview marker UUID and synthetic Entities; it never
uses the persistent desktop preview location or a user's existing profile.
Profile identities, keys, store contents, browser state, snapshots and private
control files stay within this disposable runner. All model responses come from
a loopback synthetic server; no external model account or real inference is used.

Reviewed pinned preview source lacks the `keyring` crate's `windows-native`
backend feature. The proposed Windows credential exercise therefore concerns the
profile-scoped DPAPI fallback
`R/vault.preview.<profile_id>.kek.dpapi`, together with the encrypted `master.key`,
vault sentinel and synthetic Entity birth/signing-key material. Do not describe
this as a successful native Windows Credential Manager test.
The intended preview service/account are
`com.abigail.vault.preview.<profile_id>` /
`abigail.hive.master-kek-v1.preview.<profile_id>`; these are names in source,
not evidence of native entries. The DPAPI plaintext carries the same namespace
and rejects a different profile. Each fresh profile generates a 32-byte vault
encryption key and an Ed25519 Hive master signing key. Birth also generates
Entity signing keys stored encrypted in
`identities/<entity_id>/birth_signing_key.vault`, with public keys, signatures
and birth certificates alongside the synthetic identity. These local test keys
are unrelated to SSL.com publisher credentials or model-provider API keys.

The actual released stable baseline uses its own fresh stable fixture. Reviewed
source `e3ad51181da1ea11bded24ca0c39e2dcfe5fa944` likewise enables only `crypto-rust`
for `keyring` on Windows, without `windows-native` or a replacement native
builder. Its unscoped fallback is `R/vault.kek.dpapi`; its intended native
service/account names are `com.abigail.vault` / `abigail.hive.master-kek-v1`.
These are source-derived expectations, not a native-keyring observation or
binary feature attestation. Any use of this contract is confined to the fresh
hosted runner user; the harness must never inspect or reuse the operator's
existing keyring. Both fixture scopes belong in the approval even though no
real provider credentials are involved. The recovery
test reads the existing fixture credential material; it is not cross-user or
cross-device credential portability.
Cleanup deletes the owned fixture tree and its DPAPI ciphertext after confirmed
process exit and uninstall. It never deletes Windows DPAPI system directories
or performs wildcard Credential Manager cleanup. An uncertain outcome stops
cleanup and leaves final disposal to the ephemeral hosted machine.
Windows manages its own DPAPI protection material for the disposable runner
account outside the application fixture; the harness neither reads those key
values nor removes that system-managed material. Hosted machine disposal ends
that account's test environment.

## Planned run and evidence

The runner verifies the pinned stable download and candidate signatures/hashes,
then installs both products in their fixture directories. `seed` starts stable
then preview and records synthetic identities, history, presentation and keyed
creation receipts. All owned processes stop before reinstalling the exact same
candidate bytes. `verify` starts preview then stable, checks retained state and
performs cold snapshot, verification and restore to the fresh destination.
Preview is then uninstalled; `stable-return` reopens the stable fixture before
its own uninstall.

Before any build/signing step, the workflow runs the hosted-only
`-CheckPrerequisites` path. It checks existing WebView2 registration using the
pinned Tauri NSIS detection branches, and repeats that guard before installation.
Missing, malformed or zero-version registration stops the run. Both pinned
products omit a minimum WebView2 version, so this is a presence check, not proof
of GUI compatibility. This approval scope does not include installing/updating
the WebView2 prerequisite or bypassing browser/OS warnings.

Preview teardown requires its verified HTTP shutdown acknowledgement and owned
child exit. The released stable daemon lacks that HTTP shutdown path, so its
fixture teardown uses forced termination of the exact Windows Job Object owned
by this harness. This is a required approval detail, not graceful stable
shutdown evidence. Neither path selects or kills a process by a shared basename
or a stale recorded PID.

The artifact name is `abigail-preview-candidate-<run_id>-<run_attempt>` with
14-day retention. Its allowlist contains the exact signed candidate
`Abigail-windows-x64-setup.exe`, captured signed `Abigail Preview.exe`,
`mvp-build.json`, `signature-report.json`, `windows-signatures.json`,
`prepared-build-state.json`, sanitized `preview-candidate-acceptance.json` and
`hosted-provenance.json`. The latter records both commits, hosted preparation,
recovery CLI and asset hashes, runner image and run identity, and the actual
acceptance outcome; publication eligibility stays false.

The private lifecycle state, profiles, raw logs, credential files, snapshots and
model traffic must not be uploaded. Failure reports must not include raw
exception dumps that could reveal these contents. An artifact upload preserves
candidate bytes and diagnostic evidence; it is not a GitHub release or preview
selection.

## Acceptance that remains open

This preparation has executed none of the ten [required acceptance stages](PREVIEW_CHANNEL.md#required-acceptance-evidence).
A later green workflow must report its actual assertions and gaps rather than
mark every stage passed automatically.

| Required stage | Boundary of this hosted harness |
| --- | --- |
| `separate-installer-identity` | Verify generated/installed identities and pinned payloads; source settings alone are insufficient |
| `stable-preview-coexistence` | Both launch orders and sequential install/reinstall/uninstall are planned; stable-active-during-installer behavior is not established |
| `profile-path-isolation` | Uses explicit synthetic roots; normal GUI KnownFolder bootstrap and all WebView paths remain unverified |
| `daemon-ownership-isolation` | Exercises installed-daemon routing pins and owned teardown; complete launch/race/crash/orphan coverage needs explicit evidence |
| `fresh-creation-fixture` | Uses synthetic disposable creation; normal GUI creation and the required full installed contract cannot be inferred from selected checks |
| `ongoing-entity-upgrade` | Records a first-preview seeded baseline and reinstalls the same candidate; this is not compatibility with an invented earlier accepted preview |
| `ongoing-entity-reopen` | Daemon/API history and identity checks do not prove native GUI/Runtime or WebView reopening |
| `full-cold-backup-restore` | Uses the pinned separate recovery CLI and installed daemons; successful fixture DPAPI recovery is not native-keyring, GUI or cross-device recovery |
| `newer-schema-refusal` | Requires actual byte-preserving assertions for the exact candidate; earlier component tests do not supply them |
| `return-to-stable` | Reopens the unchanged synthetic stable fixture; it does not modify or validate an operator's personal stable profile |

Native GUI launch, visible interaction, WebView-store compatibility and any
unexercised installer lifecycle cases remain separately reviewable work. No
hosted result authorizes main/beta promotion, release publication or advancing
the preview selector. Later promotion must retain the exact accepted installer
bytes and satisfy the complete channel contract.
