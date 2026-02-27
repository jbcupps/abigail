# Birth Chat vs Entity Chat: Requirements and Constraints

**Date:** 2026-02-27
**Status:** Requirements captured, implementation gated.
**Scope:** Architectural and UX differences between Birth Chat (onboarding) and Entity Chat (operational), with emphasis on the bootstrap problem and novice user experience.

---

## 1. Problem Statement

Birth Chat and Entity Chat share underlying infrastructure (the `IdEgoRouter`, `LlmProvider` trait, `AppState`) but serve fundamentally different use cases with different constraints. The most critical difference — the **bootstrap paradox** — creates a broken experience for novice users: Birth needs a conversational LLM to guide provider setup, but the user has no LLM configured yet.

This document captures the requirements for resolving these differences and ensuring both flows meet their respective UX goals.

---

## 2. Comparison Matrix

| Dimension | Birth Chat | Entity Chat |
|---|---|---|
| **Purpose** | One-time onboarding wizard: provider setup + identity creation | Open-ended operational agent conversation |
| **System Prompts** | Hardcoded per-stage (`CONNECTIVITY_SYSTEM_PROMPT`, `CRYSTALLIZATION_SYSTEM_PROMPT`) in `crates/abigail-birth/src/prompts.rs` | Dynamic constitutional prompt assembled from signed `soul.md` + `ethics.md` + `instincts.md` + skill instructions |
| **Tool System** | Text-based pseudo-tools (LLM emits `` ```tool_request `` blocks); parsed client-side | Full `SkillRegistry` + `SkillExecutor` with native function-calling format; up to 8 rounds of tool-use loop |
| **Routing** | Binary: Ego if available, else `id_only()` → CandleProvider stub | Full tier-based routing with complexity scoring, force overrides, model selection, provider fallback chain |
| **Streaming** | None — single blocking `invoke("birth_chat")` call | Full streaming via Tauri events (`chat-token`, `chat-done`) or SSE from entity-daemon |
| **Conversation State** | In-memory on `BirthOrchestrator`; lost on crash | Stateless server-side; UI manages session history and sends full context per request |
| **Side Effects** | Writes secrets, generates Ed25519 keys, signs constitutional documents, writes `soul_profile.json`, modifies `AppConfig` | Executes sandboxed skill tools; no identity/config mutations |
| **Rate Limiting** | `birth_cooldown` rate limiter | None (delegated to provider API limits) |
| **Response Shape** | `BirthChatResponse { message, stage, actions[] }` with structured action signals (KeyStored, SoulReady) | `ChatResponse { reply, provider, tier, model_used, complexity_score, tool_calls_made, execution_trace }` with observability metadata |
| **Lifecycle Position** | Runs once, before the agent has an identity | Runs indefinitely, after identity is established |

### Restrictions and Challenges

| Dimension | Birth Chat | Entity Chat |
|---|---|---|
| **LLM Availability** | May have ZERO LLM capability. CandleProvider stub returns a canned string, not a real conversation. | Guaranteed at least one provider works (birth gate at `advance_to_crystallization` enforces this). |
| **Degradation Model** | Binary cliff: Ego works (full conversation) or CandleProvider stub (dead end). No middle ground. | Graceful gradient: tier downgrade (Pro → Standard → Fast), provider fallback, CandleProvider stub as last resort. |
| **Novice User Risk** | High. User may not know what an API key is, what providers exist, or why this step matters. The chat — their primary guide — may be non-functional. | Low. Provider is already configured. Concern is response quality, not whether the agent can speak at all. |
| **Error Recovery** | App crash loses entire birth conversation (in-memory only). Interrupted birth detection exists (`check_interrupted_birth`) but resets to stage, not mid-conversation. | App crash loses nothing server-side. UI can replay session from local storage. Idempotent. |
| **Irreversibility** | High. Ed25519 keypair generation + constitutional document signing are one-shot. Repair path exists (`repair_identity`) but requires the private key or a full reset. | None. Every chat message is a fresh stateless request. |
| **Provider Discovery** | CLI provider auto-detection (`detect_cli_providers_full`) checks PATH for `claude`, `gemini`, `codex`, `grok` binaries + auth status. Useful only for power users. | Not relevant — provider already established. |
| **Stage Gating** | Hard sequential gates: Darkness → Ignition → Connectivity → Crystallization → Emergence. Cannot skip. `advance_to_crystallization` requires at least one validated provider or authenticated CLI tool. | No gates. User can send any message at any time. |
| **Pseudo-Tool Reliability** | Depends on LLM following text-format instructions (`` ```tool_request `` JSON blocks). Small/local models frequently fail to emit the exact format. The CandleProvider stub cannot emit them at all. | Native function-calling protocol supported by provider SDKs. Highly reliable with cloud providers. |
| **Waiting UX** | No streaming means user sees a loading spinner with no progress indication. Long waits (cold-start local LLM, slow provider) feel indistinguishable from failure. | Streaming tokens arrive in real-time. User sees progressive output immediately. |
| **Network Retry** | Frontend has auto-retry for connection errors (3 attempts, 1.5s delay) plus manual Retry button. Retrying a CandleProvider stub call returns the same canned message. | No special retry logic — the tool-use loop handles provider errors internally via the fallback chain. |

---

## 3. The Bootstrap Paradox

### Description

During the Connectivity stage, the system presents a conversational chat interface and a warm system prompt instructing the LLM to "guide your mentor through connecting cloud AI providers." However, the entire purpose of this stage is that the user has **no provider yet**. The routing path for a user with no Ego and no local LLM is:

1. `birth_chat` is called (`tauri-app/src/commands/birth.rs:733`)
2. `router.ego` is `None` → falls through to `router.id_only(messages)` (line 808)
3. `id_only` dispatches to CandleProvider stub (`crates/abigail-capabilities/src/cognitive/candle.rs`)
4. Stub returns: *"I need a cloud API key or local LLM to answer that. You can configure one in Settings or during the birth sequence."*

The system prompt's instructions — "Be warm, curious, and grateful" — are never read by any LLM. The user sees a chat UI that looks conversational but is actually a dead endpoint.

### User Scenario: Novice with No LLM

1. App launches → Birth starts → reaches Connectivity stage.
2. `BirthChat.tsx` mounts, sends automatic greeting: *"Hello. I just woke up."*
3. Response: *"I need a cloud API key or local LLM to answer that."*
4. User has no idea what an API key is, where to get one, or what providers are.
5. UI buttons above the chat allow manual key entry — but the user doesn't know what to enter.
6. User is stuck.

### User Scenario: Novice Who Figures Out Key Entry

1. User pastes an API key using the UI button (bypassing chat entirely).
2. Key is validated, router rebuilds with Ego.
3. `BirthChat.tsx` calls `injectKeyConfirmation` → sends *"I just saved my OPENAI API key using the button above."*
4. This message now hits a real LLM → Abigail responds conversationally for the first time.
5. The transition from dead stub to live conversation is jarring and unexplained.

### User Scenario: Power User with CLI Provider

1. User has `claude` CLI authenticated on PATH.
2. `advance_to_crystallization` detects it via `detect_cli_providers_full()`.
3. The Connectivity *chat* does not leverage this detection — user still gets stub responses.
4. User can skip to Crystallization using the "Continue" button, but the Connectivity chat never becomes conversational.

---

## 4. Requirements

### REQ-BC-001: Scripted Fallback for No-LLM State (P0)

**Problem:** When no LLM is available during Connectivity, the chat returns a canned stub message. This is the most common novice user scenario and the worst UX outcome — a chat that looks alive but is dead.

**Requirement:** When no LLM provider is configured (no Ego, no local HTTP, no CLI provider), the Connectivity stage MUST switch to a scripted, UI-driven wizard mode instead of sending messages to the CandleProvider stub.

**Acceptance criteria:**
- When `router.ego.is_none()` and `router.local_http.is_none()` and no CLI providers detected, the Connectivity UI presents a step-by-step guided flow rather than a free-form chat.
- The guided flow explains what API keys are, lists supported providers with links, and walks the user through obtaining and entering a key.
- Once a valid key is stored and the router rebuilds, the flow MAY transition to a conversational mode for the Crystallization stage.
- The guided flow must be completable without any LLM capability.

### REQ-BC-002: Streaming Support for Birth Chat (P1)

**Problem:** Birth chat is non-streaming. Long waits with no feedback (especially on first LLM contact) feel broken and are indistinguishable from errors.

**Requirement:** Birth chat SHOULD support streaming responses via the same Tauri event mechanism used by Entity chat (`chat-token`, `chat-done`).

**Acceptance criteria:**
- `birth_chat` emits `chat-token` events for token-by-token display during Connectivity and Crystallization stages.
- The `BirthChat.tsx` component renders streaming tokens progressively.
- Fallback to non-streaming for providers that do not support streaming.

### REQ-BC-003: Birth Conversation Persistence (P1)

**Problem:** Birth conversation is held in-memory on `BirthOrchestrator`. If the app crashes or user closes mid-birth, the entire conversation is lost. The user restarts birth from the stage level, losing all context from the crystallization dialogue.

**Requirement:** Birth conversation history SHOULD be persisted to disk so that it survives app restarts and crashes.

**Acceptance criteria:**
- Birth conversation history is written to a file in the data directory after each message exchange.
- On resume after interrupted birth, the conversation history is loaded and displayed in the chat UI.
- The persisted conversation is used to reconstruct the LLM context (system prompt + history) so the resumed conversation is coherent.

### REQ-BC-004: Proactive CLI Provider Surfacing (P2)

**Problem:** `detect_cli_providers_full()` identifies authenticated CLI tools (claude, gemini, codex, grok) on the user's PATH, but this information is only used as a gate check for `advance_to_crystallization`. The Connectivity chat never tells the user about detected CLI providers.

**Requirement:** When CLI providers are detected during Connectivity, the UI SHOULD proactively inform the user and offer to use them.

**Acceptance criteria:**
- On entering Connectivity stage, `detect_cli_providers_full()` results are surfaced in the UI.
- If an authenticated CLI provider is found, the UI displays a message like: *"I detected Claude CLI is installed and authenticated on your system. Would you like to use it?"*
- Accepting sets the CLI provider as active and rebuilds the router, enabling conversational mode immediately.
- This bypasses the need for manual API key entry for users who already have CLI tools.

### REQ-BC-005: Pseudo-Tool Elimination (P2)

**Problem:** Birth chat uses text-based pseudo-tools (`store_provider_key`, `recommend_crystallize`) that require the LLM to emit a specific format (`` ```tool_request `` JSON blocks). This format is unreliable with smaller local models and impossible for the CandleProvider stub. It also diverges from the native function-calling protocol used by Entity chat.

**Requirement:** Birth chat SHOULD transition away from pseudo-tools toward either:
- (a) Native function-calling via the provider SDK (same mechanism as Entity chat), or
- (b) UI-driven actions that do not depend on the LLM emitting structured output.

**Acceptance criteria:**
- The `store_provider_key` action is handled entirely by the UI (API key entry buttons, auto-detection regex), not by LLM-emitted pseudo-tool blocks.
- The `recommend_crystallize` action is detected by the UI through conversation analysis or explicit user confirmation, not by parsing LLM output for a specific JSON format.
- Removal of `BIRTH_TOOLS_DEFINITION` from system prompts reduces prompt token cost and eliminates a failure mode.

### REQ-BC-006: Graceful Stub-to-Live Transition (P2)

**Problem:** When a user stores their first API key via the UI during Connectivity, the chat transitions from CandleProvider stub responses to real LLM responses with no explanation. The tonal shift is jarring.

**Requirement:** The transition from no-LLM to live-LLM SHOULD be explicitly acknowledged in the UI.

**Acceptance criteria:**
- After a key is validated and the router rebuilds with a working provider, the UI inserts a visible system message (e.g., *"Cloud provider connected. Abigail can now respond conversationally."*).
- The first real LLM response after the transition includes context from the system prompt that acknowledges the new capability.
- If REQ-BC-001 is implemented (scripted fallback), this transition moves the user from the wizard into a conversational mode with a clear visual change.

### REQ-BC-007: Irreversibility Safeguards (P1)

**Problem:** Birth produces irreversible artifacts (Ed25519 keypair, constitutional document signatures) with limited recovery paths. A novice user may not understand the significance of the private key presentation or may accidentally close the app before saving it.

**Requirement:** The Birth flow MUST include explicit confirmation gates before irreversible operations and clear recovery guidance.

**Acceptance criteria:**
- Private key presentation requires explicit user acknowledgment (checkbox or button confirming they saved the key) before proceeding.
- Constitutional document signing displays a confirmation dialog explaining what is being signed and that it is permanent.
- If the user attempts to leave the Birth flow after key generation but before saving, a warning is displayed.
- The `repair_identity` flow is discoverable from within the app settings, not only via code.

### REQ-BC-008: Birth Duration Monitoring (P2)

**Problem:** VCB-007 targets 3-7 minutes for birth completion. The current flow has no instrumentation to measure actual duration, making it impossible to verify this target is met, especially with slow providers or the no-LLM scenario.

**Requirement:** Birth SHOULD emit timing telemetry for each stage transition so that duration targets can be measured and optimized.

**Acceptance criteria:**
- Stage entry and exit timestamps are recorded.
- Total birth duration and per-stage duration are logged.
- If any stage exceeds a threshold (e.g., Connectivity > 3 minutes), the UI offers contextual help or a skip option.

---

## 5. Entity Chat Requirements (Operational Baseline)

Entity chat currently meets its core requirements. These are documented here for completeness and to formalize the implicit contract.

### REQ-EC-001: Provider Guarantee

Entity chat assumes at least one working LLM provider. This is enforced by the birth gate (`advance_to_crystallization` requires a validated provider). No additional work needed unless this gate is relaxed.

### REQ-EC-002: Graceful Degradation Chain

The tier-based routing system (Pro → Standard → Fast → CandleProvider stub) provides graceful degradation. If the active provider fails, the router falls through to lower tiers and eventually to the stub. This chain is well-tested.

### REQ-EC-003: Streaming by Default

Entity chat streams responses via Tauri events. This is implemented and working.

### REQ-EC-004: Stateless Conversation

Entity chat is stateless server-side. The UI sends the full session history with each request. This means app crashes lose nothing server-side. No persistence requirement exists for Entity chat conversations (memory persistence for long-term recall is a separate concern tracked in the development roadmap as Phase 2c).

### REQ-EC-005: Tool Execution Pipeline

Entity chat uses the full `SkillRegistry` + `SkillExecutor` pipeline with native function-calling. Up to 8 rounds of tool-call/result cycles are supported. This is implemented and working.

---

## 6. Architectural Observations

### Shared Infrastructure, Divergent Needs

Both flows use the `IdEgoRouter`, but they need it in fundamentally different ways:

| Concern | Birth Chat Needs | Entity Chat Needs |
|---|---|---|
| **Router configuration** | May be partially configured (no Ego). Must handle missing providers gracefully as a normal operating state. | Fully configured. Missing providers are exceptional. |
| **Routing complexity** | None — binary Ego/Id selection, no tier scoring needed. | Full tier-based routing with complexity classification, force overrides, model registry. |
| **Tool protocol** | Should be UI-driven, not LLM-driven. The LLM cannot reliably drive tool actions when the LLM itself may not be available. | LLM-driven native function-calling. Provider SDKs handle format compliance. |
| **System prompt lifecycle** | Changes per stage. Hardcoded. Includes inline tool definitions. | Assembled once at request time from files + skills + runtime context. |
| **Failure meaning** | An LLM failure during Connectivity is *expected* (no provider yet). It should not produce error UX. | An LLM failure during chat is *exceptional*. Error UX with retry guidance is appropriate. |

### Key Insight

The core architectural issue is that Birth Chat is currently implemented as "Entity Chat minus features" — it uses the same router, same message format, same response parsing. But Birth Chat's requirements are structurally different: it is a **setup wizard with an optional conversational enhancement**, not a degraded version of a chat agent. The UI should reflect this by defaulting to a wizard-driven flow and enabling conversational mode as a bonus when an LLM becomes available.

---

## 7. Priority Summary

| Req ID | Title | Priority | Rationale |
|---|---|---|---|
| REQ-BC-001 | Scripted Fallback for No-LLM State | P0 | The most common novice scenario is currently broken. |
| REQ-BC-007 | Irreversibility Safeguards | P1 | Key presentation and signing are permanent; user must understand this. |
| REQ-BC-002 | Streaming Support for Birth Chat | P1 | Eliminates "is it broken?" dead-wait UX during first LLM contact. |
| REQ-BC-003 | Birth Conversation Persistence | P1 | Crash recovery for a process that can take 3-7 minutes. |
| REQ-BC-004 | Proactive CLI Provider Surfacing | P2 | Low-effort improvement for power users who already have CLI tools. |
| REQ-BC-005 | Pseudo-Tool Elimination | P2 | Reduces prompt complexity and eliminates an unreliable failure mode. |
| REQ-BC-006 | Graceful Stub-to-Live Transition | P2 | UX polish for the provider activation moment. |
| REQ-BC-008 | Birth Duration Monitoring | P2 | Enables verification of VCB-007 (3-7 minute target). |

---

## 8. Cross-References

- **VCB-001 through VCB-008:** `documents/VISION_CHUNK_01_BIRTH.md` — locked birth decisions that these requirements must respect.
- **VCC-001 through VCC-008:** `documents/VISION_CHUNK_03_CHAT.md` — locked chat experience decisions for Entity Chat.
- **Phase 2a (Skills Use):** `CLAUDE.md` development roadmap — LLM tool-use loop is a dependency for REQ-BC-005 option (a).
- **Phase 2c (Memory):** `CLAUDE.md` development roadmap — conversation persistence (REQ-BC-003) aligns with the memory persistence workstream.
