# Sprint 5 Hardening and Cutover Report

Date: 2026-03-01  
Scope: `S5-01..S5-04` from `documents/GUI_ENTITY_STABILITY_ROADMAP.md`.

## Code-First Scope Validation

- Runtime paths were traced directly in code (`tauri-app/src/lib.rs`, `tauri-app/src/commands/skills.rs`, `crates/abigail-skills`, `tauri-app/src/chat_coordinator.rs`, `tauri-app/src-ui/src/chat/TauriChatGateway.ts`) before implementation.
- Existing metadata-only policy fields were confirmed and replaced with execution-path enforcement.
- Tool-use loop bypass was addressed by enforcing policy in shared runtime (`SkillRegistry` + `SkillExecutor`) rather than command-level checks only.

## Sprint Item Closure

| Item | Status | Implementation |
|---|---|---|
| S5-01 MCP Trust Policy Enforcement | **Closed** | Added runtime trust checks via `McpTrustPolicy::validate_http_server_url` (`crates/abigail-core/src/config.rs`). Enforced at MCP registration (`tauri-app/src/lib.rs`), list-tools resolution (`tauri-app/src/commands/skills.rs`), and per-request HTTP MCP execution (`crates/abigail-skills/src/protocol/mcp.rs`). |
| S5-02 Signed Skill Allowlist Verification | **Closed** | Added `SkillExecutionPolicy` (`crates/abigail-skills/src/policy.rs`) with Ed25519 signature verification against `trusted_skill_signers`, fail-closed on invalid/untrusted signatures, strict external-skill mode when trusted signers are configured, and runtime enforcement in both activation and execution (`SkillRegistry::register`, `SkillExecutor::execute`). |
| S5-03 CLI Permission Posture Alignment | **Closed** | Added explicit runtime posture logging/warnings for CLI permission mode mismatch and variant-specific enforcement visibility (`crates/abigail-capabilities/src/cognitive/cli_provider.rs`). Updated operator docs (`documents/SECURITY_NOTES.md`, `documents/THREAT_MODEL.md`) to match actual runtime behavior. |
| S5-04 Legacy Chat Path Removal / Single Production Path | **Closed** | Removed legacy `chat-token`/`chat-done`/`chat-error` backend emissions from streaming coordinator; cut over Tauri gateway to internal message envelope (`chat-internal-envelope`) with correlation-aware handling (`tauri-app/src/chat_coordinator.rs`, `tauri-app/src-ui/src/chat/TauriChatGateway.ts`). Updated browser harness + UI tests to the envelope path. |

## Tests Added / Expanded

- `crates/abigail-core/src/config.rs`
  - `test_mcp_trust_policy_denies_non_allowlisted_host_when_strict`
  - `test_mcp_trust_policy_allows_allowlisted_subdomain`
  - `test_mcp_trust_policy_allows_any_host_when_allowlist_disabled_and_empty`
- `crates/abigail-skills/src/protocol/mcp.rs`
  - `mcp_client_denies_disallowed_host_before_request`
- `crates/abigail-skills/src/policy.rs`
  - `valid_signed_entry_allows_external_skill`
  - `invalid_signature_fails_closed`
  - `strict_signed_mode_blocks_unsigned_external_skill`
- `crates/abigail-skills/src/executor.rs`
  - `policy_blocks_execution_when_not_approved`
  - `policy_fails_closed_on_signature_regression_after_activation`
- UI adapter/cutover tests updated:
  - `tauri-app/src-ui/src/chat/__tests__/ChatGateway.parity.test.ts`
  - `tauri-app/src-ui/src/components/__tests__/ChatInterface.test.tsx`
  - `tauri-app/src-ui/src/__tests__/ChatRendering.test.tsx`

## Required Validation Gates

1. `cd tauri-app/src-ui && npm run check:command-contract` -> **PASS**
   - `Frontend commands checked: 104`
   - `Native commands registered: 147`
   - `Harness command cases checked: 86`
2. `cd tauri-app/src-ui && npm test` -> **PASS**
   - `Test Files  10 passed (10)`
   - `Tests  29 passed (29)`
3. `cd /Users/jamescupps/Repo/abigail/abigail && cargo check -p abigail-app` -> **PASS**
   - `Finished 'dev' profile ...`

Additional targeted policy checks:
- `cargo test -p abigail-core test_mcp_trust_policy -- --nocapture` -> **PASS** (3/3)
- `cargo test -p abigail-skills policy:: -- --nocapture` -> **PASS** (3/3)
- `cargo test -p abigail-skills mcp_client_denies_disallowed_host_before_request -- --nocapture` -> **PASS** (1/1)
- `cargo test -p abigail-skills policy_blocks_execution_when_not_approved -- --nocapture` -> **PASS**
- `cargo test -p abigail-skills policy_fails_closed_on_signature_regression_after_activation -- --nocapture` -> **PASS**

## Pass/Fail Summary

- Sprint 5 Implementation: **PASS**
- Required Validation Gates: **PASS**
- Policy Bypass Closure (including tool-use loop execution path): **PASS**
- Chat UX Stability (streaming render + interruption + telemetry): **PASS** (UI suites green)

## Residual Risks

1. Signed allowlist payload format is now enforced (`abigail-signed-skill-allowlist-v1`), but operator tooling for generating signatures is not yet documented as a dedicated CLI workflow.
2. `cli_permission_mode` remains a compatibility field; runtime posture is explicit and logged, but not yet a fully user-selectable enforcement model across all third-party CLI variants.
3. Legacy non-streaming `chat` command still exists for compatibility; production UI path is cut over to gateway envelope flow.

## Follow-Up Actions

1. Add a first-party signer helper/tooling doc for generating valid signed allowlist entries with the enforced payload format.
2. Add native integration coverage for cancellation race handling on `chat-internal-envelope` in Tauri runtime (beyond browser harness parity).
3. Plan and execute deprecation/removal timeline for legacy non-streaming `chat` command after compatibility window.
