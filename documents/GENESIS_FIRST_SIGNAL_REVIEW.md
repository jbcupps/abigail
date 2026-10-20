# Creation-to-return correctness review

Reviewed source: `2597127db8184aa68dbcad2edff1680a1303c076` on `dev/genesis-first-signal-integration`, AILap, 2026-10-20. This is a bounded ordinary correctness review, not a security scan or installed acceptance.

## Fixed regressions

1. **An interrupted Genesis allocation could take the legacy birth path.** The home card's `preparePresence` called generic quickstart for any unborn Entity; the Genesis workspace also offered Meet as soon as an Entity ID existed. That could replace the intended narrative with generic setup before activation finished. The real birth/open handlers now resolve both saved Genesis links and durable allocation receipts, including allocation before link persistence. Generic birth directs the user back to saved Genesis; first conversation waits for awakening. Already awakened Entities can still return while later refinements remain a draft. Legacy presentation-only Entities and receipts retain their old path.
2. **Valid canonical text could block reveal.** Genesis allows a purpose longer than the presentation sidecar's 2,000-character limit, and legacy names can exceed its 80-character limit. The card now uses Unicode-safe excerpts with an ellipsis. Canonical name/purpose remain byte-for-byte unchanged; finishing still uses the same Entity and recovery key.
3. **A valid Unicode name could leave an unrecoverable local creation journal.** Quick creation accepts 80 Unicode characters, while canonical validation previously limited names to 160 UTF-8 bytes. Canonical validation now accepts the same 80-character input and also retains previously valid legacy names. Regression tests cover 80 astral characters, rejection beyond the new boundary, and old 160-byte ASCII names.

## Real runtime review

- New creation goes through the native Hive connection and Genesis capability, not the design-preview store. Optional discovery and quick awakening share the immutable seeded session.
- Genesis reserves through the real IdentityManager receipt registry before saving the link. Same-key replay preserves later reviewed edits; unavailable allocated identities refuse replacement. Existing links remain authoritative.
- The native shell supplies profile/process ownership; browser fixture responses cannot prove this path. The real Hive router includes Genesis and the existing presence and runtime-open handlers.
- Conversation restore reads actual Entity status and `/v1/chat/history`, verifies the selected Entity, and sends with the returned session ID. The daemon chooses the latest persisted session when none is specified. Ordinary return does not reactivate Genesis or insert another awakening. The deterministic awakening turn ID keeps activation retries from duplicating that turn.
- Old visual briefs remain presentation data. The legacy UI path does not seed Genesis. The new backend guard also prevents generic setup from competing with a known Genesis creation.
- Earlier browser screenshots used a memory-only synthetic server. They do not validate signed birth, on-disk daemon restart, native IPC/WebView behavior, real inference, or full-profile recovery of the integrated build. No new native acceptance is claimed.

## Executed checks

Raw output: `output/genesis-review/`.

- Hive frontend: 327 tests passed; TypeScript/Vite build passed.
- Genesis domain: 15 tests passed.
- Hive Genesis selection: 38 tests passed.
- Creation HTTP handlers: 10 tests passed, including direct real-handler refusal before and after link persistence. Two tests overlap the Genesis selection: 61 distinct Rust tests total.
- Workspace formatting and patch whitespace checks passed.
- The initial sandboxed frontend attempt failed before running tests because Windows denied a temporary cache rename. The host run succeeded with existing installed dependencies.
- `cargo clippy --locked -p hive-daemon -- -D warnings` failed on four inherited diagnostics: unused `write_encrypted_sentinel` at `crates/abigail-core/src/vault/mod.rs:80`, and needless borrows at `crates/abigail-core/src/vault/unlock.rs:436`, `:437`, and `:478`. Both files are unchanged from the reviewed commit. This is an ordinary compiler-lint failure, separate from the advisory dependency audit; no audit or remediation scan ran. Clippy stopped in that dependency, so downstream lint cleanliness is unverified.

## CI and preview readiness

GitHub API checks for `2597127` returned no Actions runs, no check runs, and zero legacy statuses. The combined-status API reports `pending` for an empty status list; that is not a running job. CI triggers only main/beta pushes or PRs. The manual harness still pins the older product `f7c7848`; dispatching it would not validate this integration. No workflow was dispatched.

The reviewed creation regressions are fixed and source tests/build pass, but this is not a gate-clean or user-accepted preview. The finite next step is to clear the inherited compiler-lint blockers in a separately bounded change, run the required checks against the exact new dev SHA, and prepare an artifact from that same source. Then perform separately scoped disposable-profile acceptance for real creation, conversation, full process restart, and exact history/identity return before selecting a user preview. Native capture remains unavailable; installer, signing, credentials, provider calls and publication remain outside this review.

The Store worktree, previous app worktree, installed app, production profiles and running daemons are preserved. Iris receives only a project milestone/status note in `heph-status.md`; no acknowledgement loop or paused item is reopened.

## Follow-up: four compiler diagnostics

The separately authorized lint cleanup removes only three redundant borrows in `root_kek_with_backend` and marks `write_encrypted_sentinel` as `#[cfg(test)]`. Repository references confirm that wrapper is called only by the existing legacy-passphrase compatibility test; production uses the scope-aware writer. No path validation, provider ordering, cache handling, cryptography, key persistence, or recovery behavior changed. No new test is needed for the borrow coercions; compiling the core test targets verifies the compatibility helper is still available. Credential-operating tests are not executed for this cleanup.

The exact `cargo clippy --locked -p hive-daemon -- -D warnings` rerun clears all four vault diagnostics and now stops at three inherited dead-code diagnostics in unchanged `crates/hive-daemon/src/supervisor.rs`: `entity_daemon_binary` at line 67, `spawn_entity_daemon` at line 433, and `HiveSupervisor::new` at line 1016. The four-diagnostic cleanup does not expand into those helpers. Core test targets compile with `--no-run`, all 15 Genesis domain tests pass, formatting passes, and `cargo check --locked -p abigail-hive-app -p abigail-entity-runtime-app` passes without launching either shell. Follow-up output is under `output/genesis-lint/`; final exact-SHA checks are reported in the task.
