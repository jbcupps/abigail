# Exact-byte preview continuation

2026-10-20. Continue with the signed installer from
[run 39468905473](https://github.com/jbcupps/abigail/actions/runs/39468905473),
attempt 1. Do not rebuild or sign it again to obtain missing acceptance evidence.
The preview selector remains unavailable. Stable v0.0.76 remains unchanged.

| Identity | Pin |
| --- | --- |
| Product source | `f7c7848d25726480b253ec1ca637c3ffa5ea3c77` |
| Executed build/acceptance harness | `dfebfb8b589bbf3f8275eff5547e1e7c8d7b91c4` |
| Actions artifact | `12287683353`, `abigail-preview-candidate-39468905473-1` |
| Recorded archive SHA-256 | `8bf9865341e78cb2582ee1d3b261e75e46ad8e452c2bb74d211bd0923b72f6dc` |
| Installer | `Abigail-windows-x64-setup.exe`, 87,408,680 bytes |
| Installer SHA-256 | `a59580764b7cf072cf7c6eeccf040f5c934c5b0c5c164cfe5338e2ccfe5199d1` |
| Publisher thumbprint | `69BE78FD201A59A29C46F5D37097E0C6DDA04A41` |
| Original hosted provenance SHA-256 | `46d3f2f4ed7b4fc74c2f53363348b9a5f71e321a2d32bc08428f8bb7b6ead2b0` |

The successful hosted run established 20 execution steps and 16 selected
synthetic installed-lifecycle assertions, including same-candidate reinstall,
cold snapshot/fresh restore/reopen and stable return. Its sanitized reports
remain byte-identical and continue to state `publicationEligible: false`,
`fullAcceptance: false` and `realModelValidation: false`. See
[the completed run evidence](PREVIEW_HOSTED_HARNESS_VALIDATION.md#successful-signed-hosted-candidate).
The original private fixtures, snapshots and local test keys were deleted;
missing observations cannot be reconstructed from the public Boolean results.

## Importing the preserved artifact

[`import_preview_candidate.ps1`](../scripts/import_preview_candidate.ps1)
creates a **new** release-format candidate directory from the original eight
artifact files. Its inputs include saved read-only GitHub artifact/run API
metadata and explicit product, harness, run, attempt, archive, original
provenance, installer and publisher pins. It performs no network request,
build, signing, installer launch, tag creation or publication.

The importer:

- Checks the exact source file set, original hashes and metadata relationships.
- Holds source files against mutation while verifying and copying them.
- Keeps all seven original public installer/report files byte-for-byte. The
  captured standalone Hive executable remains in the original artifact, rather
  than becoming an additional release installer.
- First binds the proposed beta tag in a new `preview-candidate-import.json`.
  This is explicitly an import-time binding; it does not claim that the hosted
  build had a tag.
- Uses the existing mandatory native signature verifier and provenance tools to
  create `windows-signature-evidence.json` and `release-provenance.json`, with
  channel `candidate`. It rechecks preserved files afterward.
- Keeps publication/full-acceptance flags false. Import success is preparation,
  not release readiness.

The original archive itself is not rehashed by this importer. Its archive
digest is compared with reviewed API metadata; individual extracted files are
rehash-verified against the pinned original provenance. Do not describe this
as independent archive authentication.

The hosted baseline file was checked out with CRLF line endings. Its reviewed
Git blob and the local checkout use LF. Import allows only the exact LF or CRLF
representations of those UTF-8 bytes and records both digests and newline forms
in new metadata. Mixed newlines, bare CR and changes to content or whitespace
are rejected. Historical provenance is never rewritten to hide this difference.

The actual preserved artifact passed import and subsequent beta-format staging
with mandatory native signature/provenance verification. Seven original files
were preserved during import; staging preserved those files plus the new import
report. The proposed local tag is `v0.0.76-beta.132`; no tag/release exists as a
result of these operations. The final installer retains the hash above.
The staged `preview-candidate-import.json` hashes to
`dfe81b69b6dfb32e425fb600895000f3b4bf3544857a91842ad881a0fefac966`,
and the beta-format `release-provenance.json` hashes to
`99fa336f59dcad70f595610260c3b395bb364cc1c5f333104a68533ac8cb6e93`.

After successful import, the existing
[`prepare_preview_assets.ps1`](../scripts/prepare_preview_assets.ps1) can
prepare a separate beta-format directory with the **same proposed tag** and
signed installer bytes. That script also does not publish. Recheck that the tag
is unused before eventual publication; a local proposed tag is not reserved.

## Separate workflow and product identities

[`execution-bindings.mjs`](preview/execution-bindings.mjs) contains reviewed,
immutable workflow records shared by the browser descriptor policy and online
verifier. A descriptor cannot invent a new product checkout relationship.
Online verification checks run metadata and the exact immutable workflow blob
SHA-256; a caller-supplied claim is insufficient.

Two records are currently reviewed:

| Record | Purpose |
| --- | --- |
| `preview-candidate-39468905473` | Original candidate provenance only: product `f7c7848…`, harness `dfebfb8…`, exact run/artifact/installer/provenance pins. It cannot satisfy source CI or full compatibility. |
| `preview-source-ci-c79e214` | Source regression only: product `f7c7848…`, harness `c79e21442143829f4611e48d9ec193089b759c67`, `.github/workflows/ci.yml`, manual dispatch on `dev/preview-source-ci-0.0.76`. |

The source-CI workflow blob SHA-256 is
`72d101c7913da15d1fb7d1af610a85b3ababbb23427215426b5e1b740ba75cd9`.
All six checkout jobs verify the fixed product HEAD and caller-confirmed
dispatch revision before executing existing tests. That consistency check does
not establish harness approval. The reviewed binding plus online GitHub
run/workflow verification establishes which executions qualify as evidence;
an arbitrary green development run or offline-only descriptor check does not.
See the [PR #299 review assessment](PREVIEW_SOURCE_CI_REVIEW.md).
Ordinary push, pull-request and scheduled behavior,
permissions, required gates and advisory audit treatment are unchanged.
Source regression uses the normal build profile; it does not validate the
signed preview artifact or GUI. Existing Windows integration tests may create
disposable local encryption/signing material on their ephemeral hosted runner.
They do not use SSL.com or provider secrets and must not be repurposed as a
local test against the operator's account.

The reviewed source-CI branch is pushed. Default-branch registration is a
workflow-only patch based on main `8b68506b9443df59e486e181d81e998ec4596d62`,
on `dev/register-preview-source-ci`, initially at
`fd3f7a02ba81d5d5325f86d9e9dc3a7a5f9656ea`. The first PR-opening attempt was
rejected under the earlier dev-only boundary and did not execute. After the
parent supplied Jim's later repository authorization, one permitted retry
created [PR #299](https://github.com/jbcupps/abigail/pull/299).
Normal PR CI run `39479148145` completed: the required gate and all required
jobs passed; the existing advisory audit failed, leaving the overall run
conclusion failed. The parent held registration merge for unresolved review
comment `4329007814`; the linked assessment records the correction and proposed
disposition. Any new PR head needs its own normal checks. The separate manual
exact-product source-CI run remains pending; normal PR CI tests the PR merge
source and must not be reported as product f7 source CI.

The separately reviewed [source CI execution scope](PREVIEW_SOURCE_CI_SCOPE.md)
records the actual native DPAPI and disposable local key fixtures, the Linux
runner Secret Service lookup, and cleanup/cache boundaries. The latest
dispatch condition excludes additional credential creation; this full manual
run therefore requires specific approval. No tests are skipped to avoid that
condition, and no SSL.com signing or provider credentials are needed.

There is no reviewed full-compatibility binding yet. All ten existing
acceptance stages and the actual ongoing Entity/history/settings/seed/snapshot
observations remain mandatory. The new binding support supplies no missing
observations and does not turn the old sanitized report into a full pass.

## Validation

- Importer: 47 passed, one environment-dependent junction test skipped;
  independently rerun and reviewed. Synthetic tests mock only the adjacent
  native verifier; the real import/staging above used the actual verifier.
- Preview descriptor/online binding: 109 passed, independently rerun and
  reviewed, including combined harness substitution and legacy fallback cases,
  missing continuity and stage failures.
- Preview plus release-provenance regression: 126 passed, one Windows junction
  test skipped. These totals include the 109 preview tests, not additional
  independent acceptance cases.
- Exact-source CI policy: 125 passed, including all six extracted Bash guards,
  both approved and arbitrary matching caller revisions, and mutations that
  preserve prior required gates and ordinary triggers. The wording-only
  registration correction preserves all six guards exactly; removing manual
  additions reproduces parsed main CI at `8b68506` exactly.
- Syntax and whitespace checks passed. The checked-in unavailable descriptor
  still produces no installer link or public network requests.

## Remaining functional acceptance

The finite [remaining acceptance checklist](PREVIEW_REMAINING_ACCEPTANCE.md)
maps all ten existing stages to observed results and specific missing
assertions. It separates required visible GUI smoke from optional unsupported
claims, and records the exact available registry/UI discovery evidence.

Six exact signed-Hive preflight refusal checks passed locally: future marker,
malformed marker, duplicate marker field, newer root config, newer active Entity
config and conflicting profile environment. Each exited before initialization
with an unchanged complete fixture inventory. These are standalone preflight
checks, not normal GUI or installed daemon acceptance.

Required next evidence includes visible installed Hive/Runtime/WebView launch,
normal creation with a loopback synthetic model, the retained ongoing Entity
and visible history after reopening, and the unobserved coexistence/path/process
checks. The final fixture must capture the actual continuity observations used
by the channel contract. The first-preview seeded baseline is supported; an
imaginary earlier accepted preview is not required.

Only the recovery CLI hash was exported by the completed run; its executable
and private continuity observations were not public artifact files. Do not
claim that the original CLI bytes or deleted fixture state can be recovered
from those hashes. Any later recovery tool execution needs its actual
reviewed source/binary identity recorded separately.

Normal local GUI execution creates fresh profile encryption and signing
material. That specific disposable Newused fixture is pending approval.
Read-only inspection found no Abigail processes, but two registry opens were
denied, leaving local installation isolation incomplete. Native desktop control
is also unavailable in the tools exposed to this session. Neither an installer
nor normal local app boot was executed to fill these gaps.

Native Credential Manager support and cross-user/device recovery are not
implemented/accepted in this Windows build and are not new launch requirements.
Real provider inference is a separate untested claim; synthetic GUI checks do
not need provider credentials. The advisory audit failure stays visible and
does not become resolved by valid signatures or installed-lifecycle success.

## Publication boundary

The existing release workflow rebuilds and signs on beta/tag pushes. It is not
an exact-byte promotion path. Keep it unchanged, and do not create a tag or push
beta as a shortcut.

A later reviewed publication-only action must preserve the source CI,
compatibility and signature gates, refuse existing tag/release/asset replacement,
upload the verified exact installer as a prerelease with `make_latest=false`,
verify stable latest before and after, and verify actual public downloads before
selecting the preview descriptor. No publication action is performed by either
metadata adapter.
