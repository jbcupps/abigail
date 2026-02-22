# Prompt routing review across chat sessions

## What is happening now

- The backend chat commands build each request from a fresh system prompt plus only the latest user message.
- The UI (`ChatInterface`) preserves per-agent message history in memory (`suspendedSessions`), but this history was not forwarded to backend routing.
- Router decisions in `TierBased`/`Council` modes rely heavily on the latest message (`fast_path_classify` based on message length/keywords), so missing history causes weak context-aware routing.

## Critique

1. **Session context gap between UI and router**
   - User-visible conversation continuity exists in UI, but model routing/prompting is effectively stateless turn-to-turn.
   - This can route follow-up messages incorrectly (e.g., short follow-up after a complex thread routes local because complexity signal is lost).

2. **Low-fidelity fast-path signals**
   - Current `fast_path_classify` mostly uses message length and one keyword (`search`) for context alignment.
   - This underfits real conversational complexity and intent shifts.

3. **Prompt bloat risk if full history is later added naively**
   - Without limits, passing full history could increase latency/cost and create token-pressure regressions.

## Improvement recommendations

### Priority 1 (implemented in this change)
- **Pass sanitized session history from UI to backend chat commands** so routing and model completion can consider recent context.
- **Constrain history** with guardrails (message count + per-message char cap) to avoid unbounded prompt growth.

### Priority 2
- Add a **routing feature extractor** over recent turns:
  - cumulative tokens/chars,
  - unresolved tool-call state,
  - user intent continuity score,
  - follow-up markers (“that”, “it”, “continue”, etc.).
- Feed these features into `fast_path_classify` before target decision.

### Priority 3
- Emit a **routing trace payload** (`reason codes`) to UI for transparency:
  - `history_context_used`, `complexity_score`, `ego_unavailable_fallback`, `safety_blocked`.

### Priority 4
- Introduce **session summarization checkpoints** every N turns to compress older history while preserving intent and constraints.

## Success metrics

- Increased routing consistency for multi-turn tasks.
- Lower rate of unnecessary provider fallback.
- Stable median latency despite context inclusion.
- Fewer user corrections like “you forgot what we were doing”.
