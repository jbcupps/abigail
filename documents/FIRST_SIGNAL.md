# First Signal

An isolated, lightweight UI slice for Abigail's active Hive and Entity Runtime applications, prepared on `dev/first-signal` from `beta` at `fcde548`. Continuity work remains separate on `dev/continuity-boundary-hardening`; it is not combined with this slice.

At publication preparation on 2026-10-19, remote `beta` at `ea92eff` already contains newer, overlapping Genesis/history work. This branch still needs deliberate integration with that work and fresh integration testing. It is not merged into `beta` or `main` and is not release-ready. See [the development workflow](DEVELOPMENT.md).

## Design

An observatory rather than a wall of chat bubbles: an original luminous procedural portrait occupies its own space, with readable conversation alongside it. Three palettes (Aether, Ember, Lumen) give the user a recognizable identity that persists from creation through reveal and subsequent conversations. Sparse orbital lines and slow interference suggest presence without claiming a mind, consciousness, generated video, or lip-sync.

Creation is a short guided dialogue: intention, visual character with written response examples, then an editable review. Markdown imports produce an explicit proposal; Apply and Discard are separate actions. The allowlisted fields cannot alter permissions, providers, or executable instructions. Exports preserve field-looking and heading-looking text as quoted Markdown.

The reveal begins only after successful save, is immediately skippable, and is absent from normal replies. The portrait respects system reduced motion and offers a pause control. Text-only mode, visible microphone-off labeling, keyboard operation, and the transcript remain available.

## Implemented flow

- Hive's Entity grid opens the new creation flow and reopens saved identity briefs for editing.
- Live creation uses the existing `POST /v1/entities`, then a new `PUT /v1/entities/:id/presence`. A failed or unconfirmed second step retains the created ID for retry; navigation is held while saving. The reveal requires a valid response matching every submitted profile field, and the Entity grid refreshes before leaving the reveal. A successful save never implies that an unconnected model can answer.
- New GET/PUT presence routes persist `presence.json` atomically under the registered Entity directory. They validate registered UUIDs, enum values, text lengths, control characters, unknown fields, and a 32 KiB body limit. Corrupt saved profiles produce an error rather than a silent overwrite. The registered Entity name and runtime configuration remain unchanged.
- Hive opens an inline conversation through its existing `POST /v1/entities/:id/open` and the existing Entity HTTP/SSE gateway. Appearance restores independently, with a visible default on failure; a failed profile read cannot open an editing form that might overwrite the saved brief. Each Entity card restores independently. Navigation cancels abandoned requests and ignores late results. Hive navigation, model connections, and the running Hive helper remain available.
- Profile reads/writes have a five-second deadline; runtime opening allows 55 seconds for the existing 45-second cold-start health check. An uncertain save is reported as unconfirmed, since cancelling a client request cannot undo an accepted write.
- Standalone Entity Runtime restores appearance from Hive using its runtime status and matching session lease. An explicit Hive address skips optional lease discovery. Missing profiles fall back visibly without blocking text chat.
- The shared presence and runtime conversation source live under `entity-runtime-app/src-ui/src`. Hive imports that source and deduplicates React in Vite, avoiding divergent presence implementations or a new package/build system.

## Truthful states and media seams

| Visual state | Live source and displayed wording |
| --- | --- |
| Idle | No active request: “Ready when you are” |
| Listening form | Text field focus only: “Composing text · microphone off” |
| Processing form | Actual request: “Awaiting response”; actual token: “Receiving text” |
| Speaking form | Preview only until an actual audio playback adapter exists |
| Interrupted | User Stop or a transport interruption |
| Error | Actual transport failure |

No timer claims model reasoning. Demo state buttons are separately labeled and never instantiate a transport. Stop immediately releases the UI, preserves partial text, ignores late callbacks, and cancels when the transport handle arrives. Unmount and connection changes dispose even delayed stopped streams. Replacing a connection clears unsent drafts; empty completions display a retryable error.

`PresenceMedia` supports a portrait, state-specific video loops, posters, muted playback, and failure fallback to the procedural portrait. Playback failures are scoped to the current source and media kind, so a stale rejection cannot hide a replacement or paused clip. Sources are limited to the app origin or local blob URLs. No external asset service is called. Video honors reduced motion. `PresenceAudioAdapter` defines actual capture/playback events and stop/mute operations for later integration; no implementation acquires a microphone or synthesizes speech.

## Run locally

From `hive-app/src-ui`, install the locked dependencies if necessary, then:

```powershell
npm run dev -- --host 127.0.0.1 --port 1522
```

Hive can be installed and built independently with `npm ci` and `npm run build` in that directory. Its TypeScript paths and Vite deduplication resolve the shared presence's React dependencies from Hive; installing the Runtime frontend dependencies is only necessary when developing or building that frontend itself.

Open `http://127.0.0.1:1522/?presencePreview=1` for the clearly labeled, browser-local design preview. It creates no runtime Entity, sends no text, and saves drafts only in that origin's localStorage. Reload and Edit identity demonstrate persistence. The missing-Hive screen also links to this preview.

For actual operation, use the existing local Hive and Entity daemons rebuilt from this branch, and open the UI without `presencePreview=1`. A non-default Hive is supported through `?hiveUrl=http://127.0.0.1:PORT`. Provider/model setup remains in Hive; this slice adds no provider or credential requirements.

For deterministic transport testing without a model or personal data:

```powershell
node scripts/presence-fixture.mjs
```

Open `http://127.0.0.1:1522/?hiveUrl=http://127.0.0.1:1531`. Create an invented Entity with a purpose explicitly identifying the synthetic fixture. Normal messages yield clearly labeled synthetic responses. Send `slow` to test Stop and `error` to test failures. This in-memory loopback fixture validates the actual frontend HTTP/SSE client; it is not the real Rust daemon or an AI model. Stop its process after testing.

## Validation and limits

- Entity Runtime: 59 Vitest tests pass; TypeScript and Vite production build pass.
- Hive: 88 Vitest tests pass; TypeScript and Vite production build pass. The earlier 21-test snapshot also passed with Runtime's `node_modules` temporarily unavailable to verify independent Hive installation.
- Rust: 11 current-source tests pass: five in-process Axum route tests plus six profile storage tests against the existing real atomic writer. The production router, extraction, validation, and storage are shared directly; only registered identity lookup is synthetic. Full current daemon source and route wiring passed metadata type-checks against coherent cached dependencies. No Cargo dependencies changed. This is not a clean Cargo build.
- Browser on the Windows validation host: guided create/save/reveal, appearance restoration after reload, motion pause, text-only mode, 390 px responsive layout without horizontal overflow, real HTTP/SSE waiting/receiving/completion, immediate Stop, explicit transport failure, and subsequent recovery verified with the synthetic fixture.
- Screenshots are preserved in `output/playwright/`: creation, character selection, reveal, conversation preview, mobile view, and explicitly labeled synthetic transport evidence. See `output/playwright/validation.md` for the evidence index.

The full Rust-daemon HTTP integration test is added but was not run. Existing startup consults the OS keyring before environment overrides, and Entity creation can touch the user's Documents directory even with a separate data root. The in-process route tests cover first profile creation, save/reload after router recreation, rejected updates, corrupt storage, and failed-save retry without running startup or `POST /v1/entities`. Existing binaries must be rebuilt to expose the new presence routes. The existing entity-creation client still has no timeout/cancellation; a lost creation response requires checking the Entity list before another creation attempt. Full daemon startup/build/installer remains outside this validation.

Purpose, temperament, and boundaries are reviewable UI metadata only. They do not yet configure model instructions or enforce policy; the UI states this explicitly. Future work should connect them through a separately reviewed persona/birth contract, not inject imported text into instructions. Profile display labels do not rename the underlying registered Entity. Actual video assets, audio playback/capture, captions tied to real speech, and an audio mute control await adapters; this slice keeps audio off. Transcript persistence remains the existing runtime's responsibility, and the current frontend starts a new visible conversation when reopened.

No production deployment, full installer, paid service, new credentials, microphone permission, or changes to other worktrees were performed.
