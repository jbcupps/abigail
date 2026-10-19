# Continuity lessons applied at Abigail's existing boundaries

This change hardens two active runtime paths. It does not introduce model learning,
behavioral fitness scoring, or automatic adoption.

## Source provenance and branch scope

The separate `dev/continuity-boundary-hardening` branch carries the complete source
delta from these two local implementation commits:

- `d4edcc8518089ceea4ed826610998cf5344e43fe`: Forge proposal boundary and memory diagnostics.
- `b1f13fe0408bb84b767e56b84206079542caa6fb`: bounded Forge response correlation.

Its base is `fcde54856e8065b18c8aa0ae22ee2d23995adb32`. The publication copy preserves
that source delta while removing machine-specific audit details from this document.
The original implementation branch remains separate. This work has not been merged
with First Signal (`eed5ae63b15e07feb155a2c6141923e0ec320fb0`) or the `beta` tip observed
during publication preparation (`ea92effd31f554f74bb5e109deaa42876ec4f06f`). These dev
branches require their own review and integration validation before promotion.

## Integration and behavior

`entity-daemon` starts `soul_forge::worker::spawn_persistent_worker`, which runs the
real `DevopsForgeWorker` broker handler. That handler now accepts a single bounded
JSON object, rejects unknown/duplicate fields and malformed/truncated responses,
checks field bounds and existing static Superego patterns, and returns a typed
diagnostic. It has no skills-root argument or call to the artifact writer.

The limits are 128 KiB of encoded request bytes, 64 KiB of code, 32 KiB of markdown,
128 bytes per metadata item, and 32 keywords/topics per list. Identifiers and
filenames have a restricted ASCII grammar. Parsing does not recover JSON from
prose, fences, positional arrays, or trailing content.

Responses preserve routing independently of proposal acceptance: a valid,
unambiguous body correlation ID takes precedence over a valid transport-header
ID; otherwise the worker generates a UUID. Both sources use the same 128-byte
ASCII identifier bounds. A complete bounded JSON object is required to extract
the body ID, but rejection of other proposal fields does not erase it. Malformed,
truncated, oversized, or ambiguous correlation data falls back to the header or
UUID without relaxing proposal validation. Response body and header agree.
Correlation IDs remain excluded from diagnostic logs and confer no authority.

Wire fields such as `mentor_approved`, permissions, scores, evaluator overrides,
and adoption instructions are rejected. Broker headers cannot grant authority.
Successful responses say `status=validated`, `applied=false`,
`fitness_evaluated=false`, and `hot_reload_triggered=false`. They contain no
artifact paths. A valid repeated baseline receives static validation, not a claim
of improvement or adoption.

This intentionally replaces the prior broker behavior that trusted
`mentor_approved` from the same payload as code and immediately wrote artifacts.
The existing `process_forge_request` writer still supports explicit trusted Rust
callers, requires a non-deserializable `ForgeWriteAuthorization` argument, and
shares bounds/static validation with the broker. That argument is a caller
contract, not mentor authentication. No authenticated production approval route
has been connected to it. Its writes remain nontransactional, and its hot-reload
flag means the registry was touched, not that a watcher confirmed activation.

The separate `builtin.skill_factory` authoring tools are unchanged. These checks
do not govern every capability-creation path or prove arbitrary code is safe.

`entity-daemon` also starts `abigail_memory::subscriber::start`. That existing
subscriber still uses the real `MemoryStore`, secret routing, and asynchronous
broker path. New diagnostics exclude raw message/prompt content, context,
identifiers, paths, and freeform errors. They contain only fixed host-owned codes
and capped byte counts. A failed diagnostic write does not stop memory persistence.
Tests inject an isolated diagnostic sink without modifying global environment
variables. Existing historical logs and other observers' logging are unchanged.

## Deterministic integration fixtures

The fixtures call real Abigail components, not copied or hash-pinned subsets:

- Forge tests start `DevopsForgeWorker::spawn` with the real `MemoryBroker`.
  They exercise valid and repeated proposals, malformed/truncated JSON, trailing
  content, fences, positional arrays, duplicate/unknown fields, payload and field
  bounds, unsafe filename references, static policy blocks, and authority/scoring
  claims. Existing baseline artifact and registry bytes remain unchanged.
- A real-broker correlation fixture covers valid, policy-blocked, invalid-field,
  and authority-rejected requests with body-only, header-only, and conflicting
  IDs. It verifies precedence, size/control/type bounds, duplicate and escaped
  duplicate-null keys, malformed-body fallback, generated UUID fallback, matching
  response headers, unchanged verdicts, and identifier-free bounded diagnostics.
- Trusted writer tests require a separate host authorization argument and verify
  rejection without writes for missing authorization or out-of-bounds fields.
- Memory subscriber tests use the real `MemoryBroker` and
  `MemoryStore::open_in_memory`, backed by `PersistenceHandle::open_ephemeral`.
  Synthetic secret-shaped data is captured in the protected topic, redacted in
  ordinary stored turn content, and absent from diagnostics. Further fixtures
  verify malformed/truncated-message recovery and successful persistence when
  the diagnostic directory cannot be written.

These tests are deterministic integration checks. They do not call a model or
execute generated skill code. Rejecting a claimed aggregate score is schema
enforcement, not evidence of detecting an actual behavioral regression.

## Verification

The results below were recorded for the original source commits using an existing
local Rust build cache. They are not new test runs of the publication branch or
evidence of integration with the newer `beta` tip or First Signal. Portable command
examples below use `--target-dir target/continuity-check` to keep new outputs local
to the chosen worktree; a fresh directory may require additional compilation.

- `cargo test --offline --locked -p soul-forge -p abigail-memory --lib --target-dir target/continuity-check`:
  **46 passed, 0 failed, 1 pre-existing ignored test** (31 memory + 15 Forge).
  The ignored file-backed reopen test is already marked for SurrealKV's
  asynchronous file-lock release; the new ephemeral-store integration tests ran.
- `rustfmt --edition 2021 --check` on all changed Rust source files passed.
- `git diff --check` passed.
- `cargo test --offline --locked -p abigail-core --lib system_prompt::tests --target-dir target/continuity-check`:
  **11 passed, 0 failed**.
- `cargo check --offline --locked -p entity-daemon --target-dir target/continuity-check`:
  passed, including the updated daemon startup call and its actual dependencies.

Those recorded checks used existing Rust configuration/dependencies and build cache.
The initial sandboxed Cargo attempt could not unpack an already-cached crate;
the approved offline run succeeded. No installer, paid call, real inference,
global configuration change, or dependency download is part of this change.

For publication preparation, `rustfmt --edition 2021 --check` passed on all seven
changed Rust files, and `git diff --check` passed. Comparing the complete worktree
against `b1f13fe0408bb84b767e56b84206079542caa6fb` showed only this document changed;
the implementation and its tests are identical. Cargo tests were not rerun during
this documentation-only preparation.

## Evidence and remaining gap

Earlier local continuity research supplied the design lessons: strict proposals,
separate host authority, bounded content-free failures, and honest outcome names.
Its parser, evaluator, and diagnostic tests informed this review. Its synthetic
raw-response capture was deliberately not carried over.

The reference's reported 134 passing all-feature tests are historical evidence,
not tests rerun here. Both actual local Llama3.2 3B trials had zero adoptions: the
first response was invalid; the second repeated the baseline with no gain.
Scripted improvements are not model learning. The earlier Abigail probe tested
three pinned modules, not MemoryStore/runtime integration.

Abigail's inspected Forge path has no host-owned behavioral baseline/case suite,
no strict-gain or per-case non-regression evaluator, no authenticated runtime
application route, and no versioned adoption/rollback lifecycle. Those remain
necessary before claiming self-improvement. This change leaves broker proposals
at static evaluation only and makes that limitation visible in responses and
prompts rather than inventing scores or production adoption records.
