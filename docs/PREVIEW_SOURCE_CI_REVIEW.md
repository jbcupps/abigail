# Source CI provenance review disposition

2026-10-20. Assessment of [PR #299 comment 4329007814](https://github.com/jbcupps/abigail/pull/299#discussion_r4329007814),
thread `PRRT_kwDORLGrMs6ueixG`, review `5590093802`.
**The merge hold remains in effect pending disposition of this finding.**
No pinned-product manual CI run has been dispatched.

## Finding and acceptance boundary

The reviewer is correct about the dispatch guard. A dispatcher can select any
`dev/*` revision containing this workflow and supply its own full SHA as
`reviewed_harness_commit`. Equality with `GITHUB_SHA` checks consistency; it
does not establish independent review or require harness
`c79e21442143829f4611e48d9ec193089b759c67`. The six guards also verify the fixed
product checkout, but a green Actions result alone is not approved source-CI
evidence. The earlier workflow labels and documentation overstated this check.

Acceptance is enforced separately, using the reviewed validator revision:

1. [`execution-bindings.mjs`](preview/execution-bindings.mjs) fixes product
   `f7c7848d25726480b253ec1ca637c3ffa5ea3c77`, harness `c79e214...`, exact
   branch `dev/preview-source-ci-0.0.76`, dispatch event, workflow path and
   SHA-256 `72d101c7913da15d1fb7d1af610a85b3ababbb23427215426b5e1b740ba75cd9`.
   Descriptor input cannot add or replace that authority.
2. [`reviewedExecutionBinding`](preview/preview.mjs) rejects descriptor claims
   that differ from that record or use the candidate-origin record as source CI.
3. [`verifyPublicPreviewEvidence`](../scripts/preview_channel.mjs) checks the
   actual GitHub run repository, head, workflow, attempt, completion, event and
   branch; fetches the workflow at the immutable harness SHA; verifies its
   digest; and requires the successful aggregate gate for the exact attempt.
4. Removing the binding does not convert a manual run into legacy source CI:
   that path requires an actual beta push at the product source SHA.

The offline/browser validator checks curated claims, not actual Actions state.
An internally consistent false claim can pass that offline check. The online
CLI is required by [the promotion contract](PREVIEW_CHANNEL.md) before selecting
a preview. No automatic publication workflow currently enforces that invocation;
exact-byte publication remains unimplemented and blocked on acceptance. A
future publication action must use a separately reviewed validator revision and
require its successful online verification, not a supplied green run URL or
offline-only result.

## Narrow correction and regression evidence

The registration workflow's input description, comments and six step labels now
describe caller-revision consistency. No shell guard, checkout expression,
permission, test workload, ordinary trigger or required/advisory gate changes.
The immutable executed harness c79 and product f7 remain unchanged. Shared
binding comments and the promotion documentation now state the separate
acceptance authority explicitly.

The focused evidence tests use valid synthetic proof bytes and successful
run/gate results, while keeping product f7 and a caller input equal to the
actual run head:

| Case | Offline curated claims | Online acceptance |
| --- | --- | --- |
| Approved c79 execution | Pass | Pass |
| Arbitrary harness declared with its matching caller input | Reject | Reject: no matching reviewed binding |
| Descriptor claims c79; actual run uses another harness | Pass | Reject: actual head mismatch |
| Binding removed, another harness relabelled as a beta push | Pass | Reject: actual head mismatch |
| Binding removed, actual product-head manual run relabelled as beta push | Pass | Reject: actual event remains dispatch |

The existing tests also reject another branch/event/attempt/repository,
modified immutable workflow bytes and a failed/skipped gate. Guard policy tests
explicitly demonstrate that both the approved c79 revision and an arbitrary
matching caller revision pass the guard itself; this is not presented as an
approval check. These tests use mocked Git/fetch responses and no credentials,
dispatch, signing or product execution.

Validation completed: 109 focused preview tests passed; the combined
preview/provenance suite passed 126 tests with one Windows junction skip
(including those 109); CI policy/Bash guard tests passed 125. Parsing the
registration correction proved that all six shell guards are unchanged and
that removing the manual additions reproduces main CI at `8b68506` exactly.
An independent read-only review found no issues in this disposition or patch.

## Proposed disposition and remaining hold

The finding is valid for the guard and its former description. The tested
online acceptance path rejects the claimed substitution. The proposed
disposition is to retain that external acceptance authority and correct the
guard's description; this does **not** restrict all development CI execution
to approved harnesses. If that stronger execution restriction is required,
the present patch does not provide it and the merge hold must remain.

Hard-coding c79 only in the default-branch registration copy would not change
the workflow executed from c79. Editing that immutable harness would produce a
different commit/digest needing fresh review and binding; a commit cannot
contain its own future SHA as a literal. A trusted default-branch wrapper with
an immutable reusable-workflow reference is a separate design, not claimed by
this wording correction.

No review thread has been marked resolved. The parent must disposition this
documented acceptance boundary before merge. Separate approval for one full
manual CI run's disposable key/DPAPI fixtures is still pending; see
[execution scope](PREVIEW_SOURCE_CI_SCOPE.md). No new signing or publication is
included in either action.
