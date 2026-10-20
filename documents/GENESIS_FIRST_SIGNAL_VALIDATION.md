# First Signal / Genesis source integration

Date: 2026-10-20. Branch: `dev/genesis-first-signal-integration` on AILap.

## Reviewed inputs and decisions

- First Signal / first conversation: `e11d108dc3b44a1afca0a7f61a54ec47bb6eb737`, descending from combined preview `a4229d3609bc5146f5d60507ece96baefd8727ec`.
- Actual beta: `ea92effd31f554f74bb5e109deaa42876ec4f06f`, including Genesis and static Windows runtime work.
- Architecture A: preserve First Signal as the front door; Genesis is canonical for newly reviewed identity. Existing preview packaging, profile isolation and cold recovery remain included.

The [state/regression contract](GENESIS_FIRST_SIGNAL_CONTRACT.md) was written before implementation. New name/purpose approval is explicit. Procedural appearance, temperament examples and visual notes stay presentation metadata. Old briefs/unfinished receipts are never automatically migrated into signed narrative. Optional discovery continues the same Genesis draft. Original keyed seed input remains immutable even when subsequent reviewed identity changes.

Genesis activation now reserves through the existing durable keyed creation registry, then persists the session-to-Entity link. A failed link save leaves a recoverable reservation. Duplicate retries and manager reconstruction reuse the same UUID; unavailable allocated Entities refuse replacement. Existing linked sessions retain their IDs and ready activation is unchanged. The pre-existing deterministic awakening-turn ID and real conversation/session selection are retained.

The merged native shell binds Genesis token reads to its verified profile root. JSON and asset requests carry verified profile/process headers and refuse redirects. Creation journals additionally pin the Hive origin/profile. A verified native profile can resume its original key when that same profile restarts on a different port; different profiles and browser-origin changes are refused before dispatch. Genesis evolution uses the supervisor's checked, awaited stop with runtime-session revocation, preserving ownership checks rather than the older synchronous stop.

Packaging conflicts retain both per-app preview build overlays and beta's static-runtime import checks. Offline staging tests assert the import checker receives the four preview binaries. The earlier manual harness still pins `f7c7848`; newly inherited beta checks are excluded from that old pinned manual source. No workflow was dispatched.

## Executed validation

Successful raw logs are in [output/genesis-integration](../output/genesis-integration/).

| Check | Result |
| --- | --- |
| Hive UI component/state suite | 323 passed |
| Runtime UI suite | 107 passed |
| Both TypeScript/Vite builds | Passed |
| `hive-core` Genesis domain | 13 passed |
| `hive-daemon` Genesis | 36 passed |
| Existing creation HTTP contracts | 8 passed |
| Supervisor ownership/lifecycle fixtures | 9 passed |
| Cold snapshot fixtures | 17 passed, including unchanged Genesis capability/assets/narrative bytes |
| `cargo check` both native shells | Passed; no app launched |
| Synthetic PE/runtime inspection | 13 passed, 1 skipped (no real binary fixture supplied) |
| Mocked stable/preview staging | 22 passed; commands/PE versions mocked |
| Prepared signing-state rejection tests | Passed; no build, install or signing |

New component tests cover lost creation response, lost activation response, same-key reopen, appearance-save failure, Stop with a late response, optional discovery carrying approved changes, old presentation-only edits, native profile binding, existing Entity return, and browsing a different Entity without clearing or overwriting the unfinished creation. Backend tests cover interrupted allocation/link, restarted receipt lookup, duplicate replay, unavailable receipt refusal, legacy links/brief bytes, immutable original seed and refusal to promote old unseeded records.

The initial sandbox Rust build could not overwrite a compiler archive (Windows error 5); the authorized host build succeeded. Compilation exposed and resolved the old Genesis stop signature and the added fields in the existing HTTP fixture state. An existing unused-function warning remains; these were compile/tests, not native acceptance.

## Browser evidence

Actual source UI ran in Chrome against the explicitly labeled memory-only `presence-fixture.mjs` on loopback. Its public marker `synthetic-genesis-fixture` is fixed test data, not a real credential. No provider/model/media request occurred.

1. [Reviewed identity](../output/genesis-integration/01-review.png): name/purpose approval distinct from presentation, optional discovery visible.
2. [Reveal](../output/genesis-integration/02-reveal.png): confirmed synthetic Entity enters the First Signal reveal.
3. [First conversation](../output/genesis-integration/03-first-conversation.png): curious temperament suggests editable optional first tasks after the saved greeting. Choosing a task only filled the composer; explicit Send produced the labeled synthetic reply.
4. [Return context](../output/genesis-integration/04-return-context.png): page reload and reopen preserved the same greeting, sent question and reply, with the actual last saved question shown.
5. [Narrow return](../output/genesis-integration/05-return-narrow.png): 390px layout stayed readable without horizontal clipping.
6. [Saved draft recovery](../output/genesis-integration/06-resume-draft.png): a source reload retained Mira's unfinished creation and offered explicit resume.
7. [Discovery return](../output/genesis-integration/07-discovery-return.png): Save and exit, then resume, restored the exact unsent composer text at revision 1. Optional visual studio remains available without making it a required setup step.

Viewport override was reset; test tab and both owned loopback servers were closed. These screenshots prove source UI and same-fixture-process restoration only. The backend fixtures separately test disk receipt reconstruction and cold snapshot byte preservation.

## Remaining acceptance boundary

No production installation, user profile, running Hive/Entity daemon, provider credentials, Store work or paused remediation was changed. No beta/main push, signing, installer execution, account operation, purchase, live inference or paid media was performed. Native Windows capture remains unavailable and was not bypassed.

All five paused Store files were SHA-256 rechecked against their pre-integration values and remain unchanged. The earlier `dev/first-conversation-return` worktree remains clean. Neither existing worktree was repurposed.

This integrated source still needs separately authorized disposable-profile installed acceptance: actual signed birth with unchanged release bytes, real-model interaction, full daemon/native restart with exact session restoration, preview/stable coexistence and install/upgrade/uninstall/restore. Historical beta.130 and combined-preview evidence does not transfer to these new bytes. Generated portraits/films remain available in Genesis's optional studio; the main First Signal presence remains procedural by the chosen architecture. Release/Store/SmartScreen blockers remain open.
