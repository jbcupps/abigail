# Sprint 3 Internal Message Boundary Report

Date: 2026-03-01  
Scope: `S3-01..S3-04` from `documents/GUI_ENTITY_STABILITY_ROADMAP.md`.

## Scope Decision Notes (Code-First Validation)

- `target` semantics in desktop runtime were confirmed to be non-authoritative and effectively ignored in prior flow (`chat_stream` passed `target_mode`, but `entity-chat::stream_chat_pipeline` reserves it and does not apply target routing semantics).
- Based on current code paths, Sprint 3 applied **Option B**: deprecate/remove `target` semantics from desktop runtime internals while preserving command-payload compatibility.

## Sprint Item Closure

| Item | Status | Implementation |
|---|---|---|
| S3-01 Internal chat event envelope + normalized correlation IDs | **Closed** | Added `tauri-app/src/chat_coordinator.rs` with `InternalChatEnvelope` (`request`, `metadata`, `token`, `done`, `error`) including normalized `correlation_id` and `session_id`. Streaming path emits internal envelopes on `chat-internal-envelope`. |
| S3-02 Desktop Chat Coordinator service + thin command adapters | **Closed** | Extracted orchestration into `ChatCoordinator` and reduced `tauri-app/src/commands/chat.rs` handlers to adapter calls (`chat`, `chat_stream`) plus cancel command. |
| S3-03 `target` contract ambiguity resolution | **Closed** | Implemented Option B deprecation policy: accept `target` for compatibility, normalize to `AUTO`, and mark deprecated input in envelope metadata (`target_policy: deprecated_ignored`). |
| S3-04 Cooldown consistency for chat and birth chat | **Closed** | Added explicit chat cooldown checks in coordinator and unified cooldown error formatting via `tauri-app/src/rate_limit.rs::format_cooldown_error`, used by `chat`/`chat_stream` and `birth_chat`. |

## Tests Added/Expanded

- `tauri-app/src/chat_coordinator.rs`
  - `normalizes_session_id_and_generates_when_missing`
  - `target_policy_deprecates_and_ignores_input`
  - `envelope_contract_serializes_expected_shape`
- `tauri-app/src/rate_limit.rs`
  - `formats_cooldown_error_consistently`

Additional targeted run:
- `cargo test -p abigail-app chat_coordinator::tests -- --nocapture` -> **PASS** (`3` tests)

## Required Validation Gates

1. `cd tauri-app/src-ui && npm run check:command-contract` -> **PASS**
   - `Frontend commands checked: 99`
   - `Native commands registered: 137`
   - `Harness command cases checked: 70`
2. `cd tauri-app/src-ui && npm test` -> **PASS**
   - `Test Files  9 passed (9)`
   - `Tests  28 passed (28)`
3. `cd /Users/jamescupps/Repo/abigail/abigail && cargo check -p abigail-app` -> **PASS**
   - `Finished 'dev' profile ...`

## Compatibility and Behavior Notes

- Existing GUI streaming behavior is preserved:
  - token event: `chat-token`
  - terminal success event: `chat-done`
  - terminal error event: `chat-error`
  - interrupt flow: `cancel_chat_stream` remains unchanged from frontend perspective.
- Command surface compatibility is preserved (including `target` field acceptance).
- No experimental panel exposure changes were introduced in this sprint.

## Pass/Fail Summary

- Sprint 3 Implementation: **PASS**
- Required Validation Gates: **PASS**
- Contract/Compatibility Constraints: **PASS**

## Residual Risks

- Internal envelope event (`chat-internal-envelope`) is currently backend-only and not yet consumed by frontend/runtime analytics; drift risk remains if future consumers assume stricter payload guarantees without explicit schema versioning.
- `target` deprecation is runtime-enforced internally but frontend/public contract still carries the field; full removal requires a coordinated client contract update.

## Follow-Up Actions

1. Add explicit schema version field to `InternalChatEnvelope` before external consumption.
2. Schedule frontend/public contract migration to remove `target` from `ChatGatewayRequest` after compatibility window.
3. Add integration-level native test for cooldown behavior over sequential `chat_stream` invocations to guard regressions.
