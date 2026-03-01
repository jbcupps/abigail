# GUI/Entity Stability Roadmap

**Date:** 2026-03-01  
**Scope:** Stabilize GUI chat through a message-based flow, decouple UI from direct provider/tool APIs, and enable entity-initiated agent runs.

---

## Inputs

- `documents/GUI_ENTITY_CODE_REVIEW_REPORT.md`
- `documents/Feature_Gap_Analysis.md`

---

## Goals

1. Establish a single stable chat contract between GUI and entity runtime.
2. Remove direct GUI coupling to Tauri command details and LLM/provider internals.
3. Deliver reliable entity-initiated agent execution (agentic runs + subagent delegation).
4. Enforce release-grade command-surface and policy guarantees.

---

## Current Transitional State (Expected)

These are considered in-flight transition conditions, not final-state failures:

- Agentic command surface exists in Tauri but is intentionally stubbed.
- Orchestration scheduler exists at crate level but is not wired in Tauri runtime.
- Streaming/queue abstractions (`abigail-streaming`, `abigail-queue`) are present but not integrated into desktop path.
- Subagent manager framework exists, but no default runtime subagent definitions are registered.

---

## Immediate Stability Risks (Must Fix First)

- Frontend invokes commands that are not registered in Tauri handler in active UI paths.
- Destructive identity actions can leave runtime state inconsistent if not coordinated with active-agent/session state.
- `target` semantics in chat API are not consistently enforced across command and pipeline layers.
- Security policy fields exist (MCP trust policy, signed skill allowlist metadata) without full runtime enforcement.

---

## Sprint Backlog

### Sprint 1 - Surface Stability and Contract Guardrails

**S1-01 Command Contract CI Gate**  
Add CI validation that all frontend `invoke("...")` commands exist in Tauri `generate_handler![]`.

**S1-02 Remove/Guard Broken Invocations**  
Disable or feature-gate UI calls to currently unimplemented/unregistered command paths.

**S1-03 Identity Destructive Action Safety Fix**  
Register or remove archive/wipe paths and ensure post-action runtime state consistency.

**S1-04 Experimental Panel Flags**  
Hide unfinished agentic/orchestration panels in production runtime by default.

**S1-05 Harness Alignment**  
Keep browser harness command mocks aligned with native command registry.

**Sprint 1 Exit Criteria**
- No runtime `command not found` in default GUI flows.
- Experimental/unwired paths are not exposed by default.

**Sprint 1 Status (2026-03-01):** Completed.  
Execution report: `documents/tests/SPRINT_1_GUI_ENTITY_STABILITY_REPORT.md`

---

### Sprint 2 - Chat Gateway Abstraction (GUI Decoupling)

**S2-01 Introduce `ChatGateway` Interface**  
Define frontend chat transport contract independent of Tauri invoke/listen internals.

**S2-02 Tauri Gateway Adapter**  
Wrap `chat_stream` and token/done/error event handling behind gateway.

**S2-03 Entity HTTP/SSE Gateway Adapter**  
Implement adapter for `entity-daemon` chat endpoints.

**S2-04 Move `ChatInterface` to Gateway-only**  
Remove direct command/event coupling from UI component layer.

**S2-05 Trace/Session Contract Tests**  
Verify parity of provider/model/tier/execution-trace/session semantics across adapters.

**Sprint 2 Exit Criteria**
- GUI chat runs through gateway abstraction only.
- Both adapters pass parity tests for functional and telemetry output.

**Sprint 2 Status (2026-03-01):** Completed.  
Execution report: `documents/tests/SPRINT_2_CHAT_GATEWAY_REPORT.md`

---

### Sprint 3 - Internal Message Boundary in Desktop Runtime

**S3-01 Internal Chat Event Envelope**  
Normalize desktop runtime chat events and correlation IDs.

**S3-02 Chat Coordinator Service**  
Extract orchestration from Tauri command handlers into internal coordinator boundary.

**S3-03 `target` Contract Decision**  
Either enforce `target` semantics end-to-end or remove/deprecate it from public contract.

**S3-04 Cooldown Policy Consistency**  
Apply or remove chat cooldown path so behavior is explicit and tested.

**Sprint 3 Exit Criteria**
- Tauri command handlers are thin adapters.
- Internal chat flow is message/event-based and contract-tested.

---

### Sprint 4 - Entity-Initiated Agents

**S4-01 Wire Tauri Agentic Commands to `AgenticEngine`**  
Replace stubs with real lifecycle execution.

**S4-02 Persist and Recover Agent Runs**  
Add durable run state/history and restart recovery behavior.

**S4-03 Mentor Interaction Event Bridge**  
Expose ask/confirm/cancel lifecycle to GUI with deterministic state transitions.

**S4-04 Runtime Subagent Registration**  
Load/register subagent definitions at boot and enforce delegation policy checks.

**S4-05 Entity-Initiated Run Entry Point**  
Allow entity pipeline to enqueue/start runs without GUI-originated trigger.

**S4-06 Re-enable Orchestration UI on Real Backend**  
Connect jobs panel to actual command/runtime implementation.

**Sprint 4 Exit Criteria**
- Entity can initiate and complete agent runs with mentor checkpoint handling.
- Subagent delegation works with registered definitions in non-test runtime.

---

### Sprint 5 - Hardening and Cutover

**S5-01 MCP Trust Policy Enforcement**  
Enforce host allowlist/allow-list-only rules at runtime resolution/execution.

**S5-02 Signed Skill Allowlist Verification**  
Cryptographically verify signature/signer trust before activation.

**S5-03 CLI Permission Posture Clarification**  
Align runtime behavior and operator-facing policy documentation.

**S5-04 Legacy Chat Path Removal**  
Remove remaining direct GUI command/event chat plumbing.

**Sprint 5 Exit Criteria**
- Security policy checks enforced at runtime (not metadata-only).
- Single production chat path: GUI -> Entity message flow -> Router/Skills.

---

## Program-Level Gates

1. **Command Surface Gate**: no frontend command without backend registration.
2. **Chat Parity Gate**: trace/session parity across transport adapters.
3. **Agent Lifecycle Gate**: start/ask/confirm/cancel/complete/recover flows pass.
4. **Policy Gate**: MCP trust + signed allowlist enforcement verified in tests.
5. **Release Gate**: no open P0 stability defects for GUI chat/entity-agent paths.

---

## Ownership Model

- **Rust Backend:** command surface, coordinator, agentic wiring, policy enforcement.
- **Frontend:** gateway abstraction, panel gating, event rendering/state handling.
- **QA/Automation:** contract test gate, parity suites, restart recovery regression.
- **Release Ops:** enforce gates in release checklist and CI policy.

---

## Notes

- This roadmap complements (does not replace) broader feature-parity and vision documents.
- Until Sprint 4 completion, agentic/orchestration surfaces should be treated as experimental.
- Legacy compatibility shims (`target`, `superego_mode`, `IdPrimary` aliases) remain temporarily for client/config compatibility and are targeted for removal after the 2026-03-31 cleanup review.
