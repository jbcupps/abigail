# Real local-model check on AILap

## Verified running product

On 2026-10-21 the isolated unsigned preview from `10e5ca46c3c69080ed6fd39d809be18f9e91f39c` was connected through its normal local-provider API to LM Studio at `http://127.0.0.1:1234`. The existing downloaded `google/gemma-3-4b` weights were loaded as `abigail-preview-gemma3-4b`, with a 32,768-token context. Nothing was downloaded. No cloud model, provider credential, account grant, CLI adapter, signing service or paid call was used. Ollama was not available.

The model is the sole approved route. Actual execution traces recorded `routing_mode=approved_models`, the exact local model, successful dispatch and no fallback. Existing installed CLI status did not authorize inference and was not used to infer a working connection.

The production API check used one explicit synthetic quick-creation draft and one Entity, **Lumen Local Test**. It did not alter existing Entities. It passed:

- Durable quick draft, explicit name/purpose/personality review, activation, and duplicate activation returning the same Entity ID.
- Real conversation in 5.6 seconds, suggesting the Nebula of Sparkling Jellyfish.
- Close/reopen of only the owned test Entity through the normal Hive lifecycle API, after checking no intervening session activity.
- Exact saved history, selected session and canonical Genesis definition after a fresh Entity daemon instance.
- A real continued reply in 4.8 seconds recalling the fictional ship **Comet Teacup**.
- The UI's real SSE chat endpoint: 56 token events, a completed model trace, and the final reply saved in history. The motto was “Comet Teacup: Small Ship, Big Adventures!”

Raw synthetic-test evidence remains task-local in `preview-hands-on-10e5ca46/functional-evidence/real-model-check.json`; it is not part of the repository. Local capability tokens were consumed only in memory by the same-profile Genesis API and never logged or copied. Existing stable/older-preview daemons remained running. These are production API/process checks, not native screenshot or installed-release acceptance.

## Failures found and bounded correction

The first empty-history chat failed because stored out-of-band `mentor_monitor` records were forwarded as model message roles. LM Studio accepts protocol roles, not internal audit labels. The correction adds dialogue-only retrieval/search for chat context, filtering before limits so monitor traffic cannot displace actual conversation. General history APIs and stored monitor records retain their existing semantics; no record is deleted, no monitor is disabled, and the active system prompt remains assembled by the existing pipeline.

A regression interleaves dialogue, system and monitor records; verifies empty/absent client history yields only dialogue plus the active/recall system context; verifies older dialogue survives more monitor matches than the search limit; and confirms all original records remain stored. The running preview bytes have not been replaced by this source correction. Its successful conversation checks supplied actual saved UI history, which avoids the defective fallback path.

The initial 8K context also rejected Abigail's approximately 17,600-token full prompt. Reloading only the task-owned model at 32K resolved that rejection. Prompt size and context-aware connection checks remain a product limitation; a tiny heartbeat alone does not establish full-chat capacity.

## Remaining acceptance limits

- Optional guided Genesis received real model replies but rejected malformed typed proposal values twice. Quick/direct-edit creation passed; guided discovery is not accepted.
- Gemma once called itself “Abigail” while correctly recalling the ship. The exact phrase appears in `templates/soul.md`. In addition to the Entity-specific prompt, `entity-daemon/src/pipeline/id_stage.rs` injects `load_minimal_preprompt`, whose soul excerpt comes from this shared template (`abigail-core/src/constitutional.rs`). This is a concrete conflicting prompt source. The saved Entity identity stayed intact. Resolving the conflict needs a focused follow-up without rewriting existing signed identity documents.
- The empty-history correction requires rebuilt-runtime acceptance before it can be claimed fixed in the hands-on preview.
- No real skill execution, native visual verification, stable-profile migration, system installation or hosted release acceptance was performed.
- The preview GUI later exited while its daemons remained healthy. The cause was not established; do not call it a proven crash or a normal user close.

The same hash-verified GUI was reopened at 16:33:49 UTC against the existing isolated profile and daemons. Process/window metadata confirmed a responsive **Abigail Preview Hive**. No new profile or credentials were created, and no screenshots were available.

Source validation: 34 memory unit tests and 39 chat unit tests passed (one pre-existing memory test ignored); all six shared chat integration tests passed. Strict Clippy for `abigail-memory`/`entity-chat` with all targets and warnings denied passed. Production `entity-daemon` compile, formatting and diff checks passed. All Rust commands used `--locked --offline`. Do not infer that these live results apply to a later executable merely because it includes this source patch.

This local path does **not** satisfy the separate installed-provider requirement. Restoring a usable, explicitly approved installed-account connection remains the active priority.
