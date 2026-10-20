# First Signal on current stable product source

Integration date: 2026-10-20. Worktree branch: `dev/first-signal-integration-0.0.76`.

## Base and scope

The inspected refs were main `17b636b9e5f10b812deb287ad2c6637188991ec2`, beta `ea92effd31f554f74bb5e109deaa42876ec4f06f`, and First Signal `8b444923f8f97a01671eb5440851b6feec4e1768`. Main includes the signed stable product source `e3ad51181da1ea11bded24ca0c39e2dcfe5fa944` plus PR297's verified-publication recovery. The isolated integration starts from main so those safeguards and the current conversation persistence are preserved. First Signal's three commits are brought forward as a local patch with explicit conflict resolution. Original branches and worktrees are retained.

Beta is not an ancestor of this main tip. Its Genesis narrative/media implementation and installer/dependency changes are not silently combined here. Genesis has a different persona contract (narrative identity and structured boundaries) from First Signal's presentation palette and plain-text visual brief. A later integration must adapt those deliberately, keep existing approval boundaries, and revalidate the full release candidate. No current release/signing workflow or release-verification script is changed by this slice.

## Product behavior

The original procedural portrait remains a visual identity, with accessible text states and reduced-motion/static controls. Creation and recovery states follow actual operations and confirmed responses; chat states follow actual loading, request, token, stop, completion, and error events. No generated video, speaking, microphone capture, sentience, model quality, or permission state is inferred from animation.

The design uses a quiet observatory composition: an original light-based portrait occupies its own stage, with restrained color, generous spacing, and readable controls beside it. A deliberate reveal makes identity creation feel consequential while the persistent portrait anchors later conversations. The procedural renderer is a lightweight fallback behind media adapters, not a copied character or a claim that video assets exist.

The integrated Runtime preserves main's history restoration, session ID, cancellation, and no-automatic-replay policy. Hive preserves readiness checks, status refresh, provider setup, and its existing default setup before opening an unconfigured Entity. Imported purpose/temperament/boundaries remain presentation metadata; they are not injected into model instructions or tool authority.

## Exact creation recovery

- POST `/v1/entities` accepts optional `creation_key` (canonical UUID) with the original name. Legacy callers without a key keep their existing contract.
- A keyed request first records a reserved creation in the same durable registry as the Entity list. The agent entry and complete receipt are saved together. Replays with the same key/name reuse the same Entity. A changed name for the same key is rejected. An unavailable completed Entity is never recreated by replay.
- GET `/v1/entity-creations/:creation_key` returns an explicit `absent`, `reserved`, `complete`, or `unavailable` state. A completed receipt includes the exact Entity ID; names are not used as proof.
- The client probes this endpoint before its first POST, so an older daemon that ignores unknown request fields cannot silently disable idempotency. Retry uses the same key, original name, and original Hive. An unknown request never grants permission for a new key.
- Client waiting is bounded and cancelable. Stopping the wait does not roll back a server write. Late acknowledgements may update recovery evidence but cannot automatically save a brief or launch a conversation.
- Keyed recovery persists in browser storage across reload/closed tabs. Web Locks coordinate journal changes between tabs; unsupported browsers report the limitation instead of sending an uncoordinated create. The server's single Hive ownership and atomic registry establish the persistent idempotency guarantee; browser locks do not replace it.
- Old unkeyed drafts remain a separate manual candidate-review path; they cannot be upgraded retroactively into replay-safe requests.

The registry is scoped to the existing supported single Hive owner for a data root. Running independent Hive processes concurrently against the same root is not supported. Clearing all browser storage loses a retained client key; it does not erase server receipts. This is not cross-device draft synchronization.

Presence-profile PUTs still use the existing unconditional save contract. Browser locks prevent overlapping normal saves within one origin, and stale callbacks cannot clear or reveal a newer journal. An already accepted PUT may nevertheless finish after its client stops waiting; cross-device writes and that uncertain-write ordering need a server revision/precondition contract in a separate bounded change. The UI therefore describes stopped saves as uncertain, never rolled back.

## Verification

The complete Hive UI suite passed 264 tests; the Runtime suite passed 68. Both TypeScript/Vite production builds passed. Current-source backend checks passed 15 IdentityManager durability tests, six direct Axum creation-handler tests, three API contract tests, and 11 real profile-router/storage tests using synthetic identity lookup. Both normal `hive-daemon` and `entity-daemon` executables built successfully with locked, offline dependencies; the test-only fixed-key fixture is absent from the normal identity build. Neither daemon was launched.

Independent reviews covered the creation transaction, frontend recovery/locking, runtime integration, pinned-Hive readiness, helper restarts, and test isolation. Findings were fixed and re-reviewed: readiness after reveal, helper endpoint refresh, cross-tab journal updates, legacy session resurrection, and C1-control name rejection. No remaining concrete issue was found within those reviewed paths.

The local in-memory HTTP fixture passed 17 transport checks. Browser inspection with invented data verified creation Stop, close/reopen of the tab, recovery of the original key and brief, exact-key retry confirming the same Entity, separate brief save/reveal, model-readiness/setup/open sequencing, keyboard submission, synthetic streaming, Stop, explicit error, and transcript restoration after reload/reopen. This is UI/transport evidence, not an actual model or full-daemon acceptance result.

Current screenshots are `output/playwright/14-integrated-home.jpg` through `18-restored-conversation.jpg`; screenshots 01-13 belong to the earlier prototype. Test/build logs and the unexecuted full-daemon acceptance plan are in that same directory. The final creation-status spacing fix is included in the builds; the earlier creation screenshots precede that small margin correction.

Full-daemon HTTP acceptance is held because normal bootstrap consults the OS keyring before the raw test-key override. The plan-only harness in `scripts/tests/first-signal-acceptance.mjs` explicitly prevents launch until a supported isolated startup is reviewed. No credential-provider code or protections were changed to get around that limitation.

The source-only [startup test isolation proposal](STARTUP_TEST_ISOLATION_PLAN.md) records the exact credential call path, existing injection seams, and a bounded test-only router assembly plan. That proposal has not been executed and would not establish normal process/bootstrap acceptance.

The final mobile browser inspection remains incomplete. A temporary viewport-emulation request was paused for platform approval; its browser call later returned, but the parent-directed hold remains authoritative until the approval outcome is confirmed. No mobile screenshot or mobile-layout pass is claimed for this integration.

After validation and independent review, committing and pushing the isolated development branch was authorized. This snapshot is development evidence, not release acceptance. No main/beta merge, tag, release, installer, new provider credential, or deployment is part of that publication. Release workflows, verification scripts, and Cargo.lock remain unchanged.

## Next bounded slices

1. Reconcile this reviewed slice with beta Genesis through an explicit profile/narrative adapter and one agreed creation path. Preserve stable signing/release safeguards and current transcript persistence. Test and review the combined candidate before proposing promotion.
2. Implement a separate local dynamic-API skill review/activation flow using the existing parser, registry, executor, watcher, and bus. Preserve version/digest, evaluation evidence, explicit approver, and last-known-good rollback; drafting alone grants no authority. The source map distinguishes actual mechanisms from gaps in [BUS_AND_SKILL_SOURCE_MAP.md](BUS_AND_SKILL_SOURCE_MAP.md).
3. Keep actual video/audio assets, capture/playback events, and durable cross-process bus delivery separate. The current daemon uses MemoryBroker; SurrealDB supplies persistence, not a substitute bus. Do not introduce Iggy from roadmap notes alone or connect Iris's skill library without a separate scoped decision.
