# First conversation and reliable return

Validated on AILap on 2026-10-20. Development branch: `dev/first-conversation-return`.

## Source selection and integration boundary

Fresh remote comparison used beta `ea92effd31f554f74bb5e109deaa42876ec4f06f` and combined preview `a4229d3609bc5146f5d60507ece96baefd8727ec`. This isolated worktree starts at the combined preview. It already contains First Signal creation and appearance, durable keyed creation recovery, restored conversation sessions, profile isolation, cold recovery, and preview packaging evidence. This change extends that implementation instead of recreating it.

Beta is divergent: Entity Genesis (`30282bb`) adds its own creation/media/voice surface and daemon APIs; the standalone Windows runtime work (`33aa0eb`) includes static CRT and runtime verification; installer lock protection is in `dfe8524`. Those changes remain untouched on beta and in the paused Store checkout. **They are not merged into this branch.** A source-only merge-tree comparison found conflicts in Hive App, daemon routes/state, release scripts/docs, and CI. A whole-branch promotion to beta would require a separate deliberate integration and backend/native validation. Do not replace beta with this tree or describe this branch as containing Genesis/static-runtime integration.

This patch changes only the shared Runtime conversation UI, its styles, and tests, plus these evidence artifacts. Hive's inline conversation uses the same component. No daemon, creation route, startup/profile binding, installer, signing, workflow, or provider configuration changed. No beta/main ref was updated. A dev-branch push does not trigger the checked-in signing/release workflows.

## Implemented behavior

- Optional first missions follow the saved gentle/direct/curious temperament. They remain visible after an initial persisted assistant greeting. Choosing one fills and focuses an editable draft; it does not send, modify runtime instructions, or replace an existing draft.
- Request progress distinguishes waiting for first text, a 12-second wait without text, and actual arriving tokens. Status is announced outside the busy transcript. No percentage, model reasoning, or tool success is invented.
- Stop preserves partial text and the next draft, ignores late callbacks, and says remote work may still finish. The browser exposed an existing button-reuse bug: Stop could become a submit button before the click's default action and send a waiting draft. Separate button keys plus `preventDefault` fix it; focus returns to the composer.
- Check saved conversation uses the existing status/history GETs only. It never replays a chat POST. It preserves a draft while loading, keeps visible text on failure/timeout, rejects changed identity/session, and ignores stale responses after connection changes. It can be repeated because cancellation is best effort.
- Reopening shows a bounded, verbatim excerpt from the last saved user message. It never labels optimistic local text as saved context. The continue action only focuses the composer.

## Verification

| Check | Result |
| --- | --- |
| Runtime frontend `npm test` | 107 tests passed, 7 files |
| Hive frontend `npm test` | 283 tests passed, 11 files |
| Runtime `npm run build` | TypeScript and Vite passed |
| Hive `npm run build` | TypeScript and Vite passed |
| `git diff --check` | Passed |
| Browser creation → optional starter → scripted reply → reopen | Passed against repository HTTP fixture |
| Browser slow stream → draft while streaming → Stop | Partial retained; draft retained; no new request after fix |
| Browser saved-history check and scripted error | Clear state; no automatic message replay |
| Narrow-window check, 390 px | No horizontal overflow; recovery and composer remain available |
| Pause motion / text-only controls | Status and composer remain available |

New tests cover all three starter personalities, persisted greetings, draft protection, saved-only context, delayed response timing, Stop with an unsent draft, read-only reconciliation with the real HTTP gateway, stale replies, changed identities/sessions, timeouts, and connection replacement. Existing creation receipts, restoration, isolation, gateway, and preview tests remain passing.

The initial sandboxed Vitest run stopped before running tests because its temporary-file rename returned EPERM. The recorded successful runs used approved host execution. The first sandboxed dev server was unreachable from Chrome; only the isolated loopback fixture/dev server were restarted with approved host access. No OS settings or running installed daemons were changed.

## Visual evidence and limits

Screenshots use `scripts/presence-fixture.mjs` on loopback port 1631 and the worktree's Hive UI on port 1641. All data are invented and memory-only. The synthetic banner and replies explicitly identify this. The browser's viewport override was reset after the narrow-window check. The fixture and dev server are stopped after validation.

- `01-first-task.jpg`: optional curious first missions.
- `02-reopened-context.jpg`: restored transcript and last saved question.
- `03-request-progress.jpg`: progress driven by actual fixture stream callbacks.
- `04-stopped-partial.jpg`: corrected Stop behavior with partial text and an unsent draft.
- `05-rechecked-draft.jpg`: saved snapshot loaded without sending the draft.
- `06-error-recovery.jpg`: actual scripted transport failure.
- `07-narrow-recovery.jpg`: narrow layout with recovery and composer.

These are source-UI and synthetic transport checks, not native installed acceptance, live model quality, tool/skill execution, or disk persistence across daemon restarts. Native Windows screen/window capture remains unavailable in this execution context: the computer-use skill's required `node_repl`/`@oai/sky` API is not exposed, and CUA exposes browser control with native APIs disabled. Existing SmartScreen/signing/Store release blockers are not resolved by this slice.
