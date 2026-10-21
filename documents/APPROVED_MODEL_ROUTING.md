# Approved model routing

## Contract

Discovery -> explicit connection check -> saved connection -> user-approved ordered set -> readiness filtering -> bounded dispatch. Discovery, saved credentials, Entity preferences, model overrides and model output cannot add to the approved set. An empty set means no model. Adding a connection explicitly approves that connection; Settings can narrow or reorder the set without reconnecting accounts.

Hive owns the policy. Genesis and Entity runtime use the same dispatcher. Entity configuration cannot override the household set. Each attempt uses the connection's approved model and explicit local endpoint; no ambient provider scan, guessed local server, synthetic response or unrelated provider fallback is permitted. Local-only background requests stay local. Runtime dispatch rechecks Hive approval before each attempt, so revocation applies before the next model call; already-running inference cannot be undone.

At most three approved connections are attempted once each, in saved order, with a bounded attempt timeout. Failed connections enter a short cooldown. Streaming never changes provider after output has been emitted, and a closed output channel cancels further attempts. The execution trace records the actual attempted provider/model. Readiness means a previously checked connection can currently be constructed; expired account access or network failure remains a real error, not a claim of live availability.

Legacy profiles retain their explicitly saved primary choice only; old credentials and local fallback fields are not implicit consent to additional routes. Identities, drafts, receipts and sessions are not migrated or rewritten. Unsupported preview CLI account access stays blocked.

## Regression requirements

- Unapproved and unready connections never receive requests.
- Empty policy and missing Hive approval fail closed without synthetic success.
- Timeout/error permits only bounded approved fallback; partial streaming and cancellation stop fallback.
- Caller model overrides cannot select an unapproved model.
- Entity preference changes and provider-profile requests cannot widen Hive policy.
- Genesis shares dispatch behavior; revocation is checked before runtime attempts.
- Saved choices survive reopen; unavailable approved connections remain visible with recovery actions.
- First-run setup remains postponable, preserves unfinished creation, and returns to the Entity home; Settings owns later changes.

All validation uses fixtures. No live provider calls, grants, credentials, installed-app changes or preview replacement are part of this implementation.

## Implementation and limits

- The existing Hive config stores a nonsecret catalog and ordered choices. Explicit connection checks approve only the chosen route; Settings can narrow or reorder it. Concurrent connection checks/Settings writes use an expected-policy comparison under a shared write lock, preventing stale setup from restoring revoked choices.
- Hive's private provider handoff contains only resolved approved routes. Production Entity startup and governance refresh use `build_router_guarded`; each attempt reads current Hive approval. Subagent hints and legacy router entry points cannot build alternate providers. Missing approval fails closed. Added choices propagate through the existing approximately ten-second refresh; revocations are checked before every new attempt. Already-running inference cannot be recalled.
- Genesis uses the same dispatcher and a direct Hive approval gate, with tools disabled. Runtime execution traces identify actual attempts and the final provider/model. Genesis retains its existing reply schema; it does not add a per-turn provenance UI in this slice.
- At most three attempts, 30 seconds each, plus bounded approval reads; failed routes cool down for 30 seconds in an existing dispatcher. Genesis constructs a dispatcher per turn. This is ordered availability-based routing, not a cost/quality planner.
- Readiness is configuration eligibility, not a continuous live-authentication or model-service guarantee. Connection checks and actual requests determine real availability. Local routes retain their explicit endpoint/model and can recover after an offline start; no guessed endpoint or embedded synthetic model is used. A legacy local choice without a saved model still queries its explicitly approved server for its current model until rechecked in setup.
- Unsupported Gemini CLI and isolated-preview CLI account restrictions remain. Consumer apps and browser sessions are never treated as model APIs. API, Ollama and LM Studio routes remain supported.
- No dependencies, workflow pins, prepared executables, installed applications, profiles, credentials or Store files are changed. No real account/model invocation is part of validation.

## Validation on AILap

Validation uses source components, disposable fixtures and loopback HTTP only. Raw logs and `07-approved-models.jpg` are kept in ignored `output/provider-readiness/`. The browser fixture intercepted every request and saved only in-memory choices. Settings navigation, selection/reordering and success feedback were visually checked; a contrast issue was corrected. The temporary tab and source server were closed. This does not establish native-installed or real-account acceptance.

Completed validation (48 focused Rust tests, 454 frontend tests):

| Check | Result |
| --- | --- |
| `cargo test -p abigail-core --lib model_routing::tests --locked --offline` | 3 passed |
| `cargo test -p abigail-hive --lib approved_dispatch::tests --locked --offline` | 7 passed, including actual local HTTP recovery and exact model pinning |
| `cargo test -p entity-daemon router_build::preview_provider_tests --locked --offline` | 5 passed, including production runtime/Hive approval recheck before actual loopback dispatch |
| `cargo test -p abigail-router --lib router::tests --locked --offline` | 20 passed |
| `cargo test -p hive-daemon routes::tests --locked --offline` | 12 passed |
| `cargo test -p hive-daemon genesis_director::tests --locked --offline` | 1 passed |
| Hive UI `npm test -- --run` / `npm run build` | 347 passed / build passed |
| Runtime UI `npm test -- --run` / `npm run build` | 107 passed / build passed |
| `cargo check -p abigail-hive-app -p abigail-entity-runtime-app --locked --offline` | passed |
| `cargo clippy -p abigail-core -p abigail-hive -p abigail-router -p hive-daemon -p entity-daemon --all-targets --locked --offline -- -D warnings` | passed |
| `git diff --check` | passed |

The prepared `a18249a3` build-state SHA remains `99B03F9ED594BDA687BFF6CC1DD724ECA9223502A6C0370BC1AEA5AD780B6C9F`; all five paused Store-file hashes also match their recorded values. No claim of a full backend suite or hosted CI is made from these focused checks. A new isolated preview build, followed by authorized real-model creation/conversation/reopen acceptance, remains the next product step. The current hands-on preview was not replaced or restarted.
