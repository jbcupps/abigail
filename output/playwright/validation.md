# First Signal validation

Validation recorded on a Windows host, 2026-10-19, for the isolated First Signal source based on `fcde548`. This evidence accompanies development branch `dev/first-signal`; it is not validation of a merge with newer `beta` changes.

The original checkout and its existing changes were preserved. Continuity work is maintained separately on `dev/continuity-boundary-hardening`; its backend changes were not merged into First Signal. At publication preparation, remote `beta` at `ea92eff` contains newer overlapping Genesis/history work. Integration with that work is still required before this slice is release-ready.

## Automated checks

- Entity Runtime: `npm test -- --run` — 44 tests across 4 files passed.
- Hive: `npm test -- --run` — 21 tests across 3 files passed.
- Both applications: `npm run build` — TypeScript and Vite production builds passed.
- Hive also built and passed all 21 tests with Runtime's `node_modules` temporarily unavailable, confirming its independent install path. Runtime dependencies were restored afterward.
- Profile persistence: 6 current-source Rust storage tests passed, using the real existing atomic writer. Full current daemon source/route wiring passed metadata type-checks against coherent cached dependencies. See `backend-validation.txt` and `backend-check-source.ps1`. The script requires explicitly supplied cached Rust dependency and native-library directories from a matching build; this is not a clean Cargo build.
- `node --check scripts/presence-fixture.mjs` and `git diff --check` passed.

## Publication rechecks

The following checks were repeated in the dedicated `dev/first-signal` worktree during publication preparation on 2026-10-19:

- Hive: all 21 tests passed; TypeScript and Vite production build passed.
- Entity Runtime: all 44 tests passed; TypeScript and Vite production build passed.
- `rustfmt --check` passed for the four changed Rust files: `main.rs`, `presence.rs`, `presence_profile.rs`, and `tests/integration.rs` under `crates/hive-daemon`.
- `node --check scripts/presence-fixture.mjs` passed.
- The parameterized `backend-check-source.ps1` storage check passed all 6 tests with explicitly supplied matching dependency and native-library directories.

The original commit and its worktree were left unchanged. These checks validate the First Signal development snapshot, not a merge with newer `beta` changes. The full daemon metadata checks above remain historical cached-graph evidence; the real daemon HTTP integration test remains unrun.

## Reliability follow-up

Current-source checks after the reliability changes on 2026-10-19:

- Hive: all 88 tests across four files passed; TypeScript and Vite production build passed.
- Entity Runtime: all 59 tests across four files passed; TypeScript and Vite production build passed.
- Actual in-process Axum route/storage suite: 11/11 passed, including five route integration cases and six storage unit tests. The tests import the current production router/profile modules; only registered identity lookup is synthetic. No TCP listener, daemon startup, provider, model, or actual Entity creation is involved.
- Full current `hive-daemon/main.rs` metadata compilation passed with the coherent read-only cached dependency graph. Rust formatting and whitespace checks passed. No dependencies or lockfiles changed.
- Frontend coverage includes profile request deadlines/cancellation, exact save acknowledgements, independent card restoration, chat access during failed/delayed appearance restoration, failed edit protection, direct home navigation from the reveal, explicit Hive address restoration, media replacement/pause failures, late stream teardown, and empty-response recovery.

The cached-graph reproduction script is `crates/hive-daemon/tests/check-presence-routes.ps1`; supply matching dependency and native-library directories. It writes outputs and synthetic temporary directories only beneath this worktree's ignored `target/presence-route-check`. This is not a clean Cargo build or a full daemon startup test.

## Browser evidence

The Chrome preview used `http://127.0.0.1:1522/?presencePreview=1`. It saves identity drafts in that browser origin only, creates no runtime Entity, and does not send conversations.

| Screenshot | Verified view |
| --- | --- |
| `01-first-signal.jpg` | Name and purpose step with original procedural portrait |
| `02-character.jpg` | Lumen visual identity and Curious written temperament example |
| `03-reveal.jpg` | Successful save followed by skippable reveal |
| `04-conversation-preview.jpg` | Persistent portrait, text fallback, visibly labeled manual demo states |
| `05-mobile.jpg` | 390 px layout; document width equaled viewport width, no horizontal overflow |
| `06-http-streaming-fixture.jpg` | Actual frontend gateway receives scripted SSE tokens |
| `07-http-error-fixture.jpg` | Explicit error after SSE and JSON fallback failures |
| `08-http-recovery-fixture.jpg` | Subsequent request completes after error |
| `09-saved-grid.jpg` | Newly saved Vesper appears after direct home navigation from reveal, without Refresh |
| `10-restored-presence.jpg` | Vesper's Ember appearance and synthetic purpose restored after reload; motion paused |

Created Lyra, saved, reloaded, and reopened the same appearance. Checked pause, text-only mode, responsive layout, and keyboard Enter submission. The temporary viewport override was reset.

The follow-up browser review created Vesper using invented, explicitly synthetic text, chose Ember/Direct, saved, returned directly from reveal to Your Entities, reloaded, and reopened the preserved appearance. Screenshots 09/10 are browser-local design preview evidence; no runtime Entity or model turn was created. The same local preview remains available for review.

The separate transport browser used `?hiveUrl=http://127.0.0.1:1531` with `scripts/presence-fixture.mjs`. The invented Entity's visible purpose identifies it as a synthetic fixture. Creation, profile save, open, streaming completion, Stop, failure, and recovery traversed the actual frontend API/gateway against this memory-only loopback server. No AI model, provider, or real Rust HTTP daemon was called. The fixture server was stopped after validation.

## Remaining integration work

The added real Rust HTTP integration test has not run. Existing daemon startup reads the OS keyring before configuration overrides, and creation can write the user's Documents directory even with a separate data root. We stopped before launching it; this was a scope/isolation choice, not a denied operation. Rebuild the existing daemons and run the gated HTTP test in a suitably isolated environment before release.

The later direct in-process Axum suite passes without those startup side effects. Its initial loopback-client approach encountered Windows socket error 10013 and was abandoned without escalation; the successful tests call the same production Router with HTTP Request/Body objects directly. The existing Entity-creation client still lacks timeout/cancellation; an unknown creation result requires checking the Entity list before retrying.

Video and audio adapters are interfaces and local-media fallbacks; this delivery uses procedural visuals with audio off. Purpose, temperament, and boundaries remain explicitly labeled metadata, not runtime model instructions or enforced policy. No production deployment or installer was performed.
