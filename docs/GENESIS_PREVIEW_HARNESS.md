# Exact Genesis preview harness

Product: `a18249a3b622e8b34d0f20ec1f0e24fc0dc7183d`.
Execution branch: `dev/genesis-preview-harness`. This separate worktree starts
from that product commit, changes only harness source/tests/documentation, and
does not alter the prepared product checkout or its unsigned artifacts.

## Review boundary

Both hosted workflows check out the immutable product above. Candidate policy,
runner default, stable-comparison baseline and lifecycle contract use the same
pin. Stable fixture pins are unchanged. Manual CI retains its six checkout
guards and every existing job, permission, required gate, advisory gate and
ordinary trigger. Beta's Windows dependency regression, NSIS prerequisite and
installer-lock regression now also run for this newer manual product; the old
product-specific skips are removed.

Caller input equal to the workflow revision proves consistency, not approval.
After freezing the execution commit, `dev/genesis-preview-evidence` records its
exact SHA and Git-blob workflow hashes in the existing reviewed binding table.
Use that separately reviewed validator revision for future evidence acceptance.
Neither record grants execution/signing permission or resolves PR #299's review
hold. Historical f7/c79/dfeb bindings and actual run/artifact records remain
unchanged; they cannot validate this product. A new workflow binding is not a
new run, artifact or compatibility acceptance record.

Candidate booleans still default false. Hosted-run restrictions, one-pass
signing, serialized signing, separate product/harness checkouts, stable fixture
verification, isolated profiles, exact allowlisted upload and provenance fields
are preserved. No signed bytes or new hosted run are claimed here.

## Registration and next operations

Read-only observation on 2026-10-20:

- Main `8b68506b9443df59e486e181d81e998ec4596d62` has no `ci.yml`
  `workflow_dispatch`. PR [#299](https://github.com/jbcupps/abigail/pull/299)
  remains open, head `e2724672775803f2ecf1af005c31a31936c1ff98`.
  [Run 39487321911](https://github.com/jbcupps/abigail/actions/runs/39487321911)
  passed the required gate; advisory audit failed. No manual source-CI run
  exists. Workflow API registration alone does not establish a manual trigger.
- Parent must disposition [the documented review hold](PREVIEW_SOURCE_CI_REVIEW.md)
  and separately authorize/merge the registration change before manual CI can
  dispatch this frozen dev harness with its exact SHA as
  `reviewed_harness_commit`. No product merge into main/beta is required.
- Ordinary PR CI can run without merging, but tests the PR merge revision,
  not this immutable product. A dev push alone triggers no CI. No PR or workflow
  is created/dispatched by this preparation.
- Candidate workflow ID `390817899` is already registered. Once its exact
  execution scope is approved, it can dispatch from the frozen dev branch
  without another registration merge. It builds fresh hosted bytes; it does
  not sign or reuse the locally prepared artifact bytes.

Full source CI is nonsigning but runs existing disposable identity/DPAPI tests,
dependency installation, advisory audit/CodeQL and the newer NSIS regression.
The latter builds/runs synthetic installer/uninstaller lock fixtures under a
unique test root and HKCU fixture key; it does not install actual Abigail.
This turn does not execute those operations. The source-CI approval scope in
[PREVIEW_SOURCE_CI_SCOPE.md](PREVIEW_SOURCE_CI_SCOPE.md) needs this explicit
newer-workload addition before a run.

Candidate execution needs separate approval for its disposable stable/preview
identities/DPAPI, isolated install/upgrade/uninstall and owned-process teardown,
plus its signing pass. Approval for six local signing requests alone does not
authorize that full hosted run or another six hosted requests. Allowance is
unverified. No secret values are needed for source review.

## Acceptance limits

Local validation is policy/guard execution and mocked evidence/lifecycle tests.
No product/installer launch, credential operation, network model, security scan,
signing, dispatch, release, selector update or main/beta merge occurs.
The candidate lifecycle still exercises synthetic daemon/legacy creation; it
does not establish Genesis GUI creation, normal WebView reopen, real provider
behavior or all exact-installer acceptance stages. New product source tests,
hosted source CI, candidate-origin proof and native installed acceptance are
distinct evidence. Native capture remains unavailable on this agent.

## Frozen execution and local validation

Harness commit: `0bb0f4fd0cd181250cc62ed472699c5ea21c79a2`, retained on
`dev/genesis-preview-harness`. The companion `dev/genesis-preview-evidence`
branch contains this document, the two reviewed bindings and byte-exact workflow
fixtures. Dispatch the execution branch, not the later evidence branch.

| Git blob at frozen harness | SHA-256 |
| --- | --- |
| `.github/workflows/ci.yml` | `4114e030cab0eda10995884e746a0bffeed73eba373eeb34cbf02345aa171022` |
| `.github/workflows/preview-candidate.yml` | `d9ccd63dc36ded79b34cced601c9eb2a12b2f9df0249c8cc89b69ff349ee0886` |

Local results on AILap:

- Candidate workflow policy/parser/scope: 50 passed. Pure runner: 111 passed;
  its synthetic `D:` fixture needs a process-local PowerShell PSDrive on this
  C-only machine. This mapping is not persistent and creates no OS drive.
- Owned-job helpers: 66 passed, including C# declaration compilation without
  invoking native process APIs. Synthetic lifecycle: 38 passed.
- Preview/provenance evidence: 155 passed, one existing Windows-junction test
  skipped by the filesystem sandbox. Both historical and new source bindings
  reject altered branch/event/product/harness/workflow/attempt/gate evidence.
  The new candidate workflow record cannot manufacture artifact acceptance.
- Exact text comparison to immutable a182 CI confirmed only the listed pins,
  labels and three manual skips changed. All six actual Bash guards passed 54
  mocked cases (no real Git/product calls). Git Bash needed the normal approved
  sandbox escalation to initialize its MSYS object namespace.
- Node syntax, PowerShell parser checks, default plan-only runner and Git diff
  whitespace checks passed. No product changes require a new product build.

**Unverified local check:** `node --test scripts/tests/test_preview_source_ci.mjs`
stops before its cases because both available Python runtimes lack PyYAML
(`ModuleNotFoundError: yaml`). No package was installed. The exact-baseline and
54 guard checks above do not substitute for this parsed policy suite. Rerun with
`PREVIEW_CI_TEST_PYTHON` pointing to an approved Python+PyYAML environment and
Git Bash before declaring all harness tests green. Its historical workflow
digest is preserved; the three newer required steps are asserted separately,
with mutation tests for omission and manual-dispatch suppression.
