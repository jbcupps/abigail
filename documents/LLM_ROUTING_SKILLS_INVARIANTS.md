# LLM Routing and Skills Sequencing Invariants

This file captures the runtime invariants used to verify routing and capability
execution correctness across chat, birth, forge, CLI, and skills commands.

## Entry Invariants

- All LLM-facing commands (`chat`, `birth_chat`, CLI chat endpoint)
  must route through a configured `IdEgoRouter` instance.
- Router rebuilds after config changes must update both:
  - active router state, and
  - subagent manager router reference.
- `ChatResponse` must include `execution_trace` as the authoritative source
  of routing attribution. The `tier`, `model_used`, and `complexity_score`
  compatibility fields are derived from the trace, not computed independently.

## Attribution Invariants

- `ExecutionTrace` is the single source of truth for what provider, model,
  tier, and selection reason were used for each turn.
- `ToolUseResult` exposes `.tier()`, `.model_used()`, `.complexity_score()`
  accessor methods that derive values from the embedded `execution_trace`.
- Response construction (in Tauri commands and daemon routes) must extract
  attribution from `execution_trace` accessors before moving the trace into
  `ChatResponse`.
- `last_provider_change_at` is updated only when the ego provider actually
  changes, not on every router rebuild.

## Routing Invariants

- `tier_based` classifies message complexity (score 5–95) and maps to a model
  tier via `TierThresholds` (default: <35 → Fast, 35–69 → Standard, ≥70 → Pro).
- `ego_primary` targets Ego only when Ego client is actually available; otherwise
  it must fallback to Id (local LLM). Uses the Standard tier model.
- `council` is an explicit execution path that delegates to `CouncilEngine`
  when 2+ providers are enrolled; degrades to single-provider passthrough
  otherwise. Council failures fall back to ego/id with `SelectionReason::Fallback`.
- Local LLM (Id) is failsafe only — all primary routing goes to cloud (Ego).
  Id activates only when the Ego provider returns an error.
- Provider status reporting must never claim an Ego provider when the client
  failed construction.
- Each routing decision records a `SelectionReason` enum: `Complexity`,
  `PinnedTier`, `PinnedModel`, `SetupIntent`, `EgoPrimary`, `Council`, or
  `Fallback`.

## Force Override Invariants

- `ForceOverride.pinned_model` has highest priority — bypasses all tier logic
  and sets `CompletionRequest.model_override` directly.
- `ForceOverride.pinned_tier` (+ optional `pinned_provider`) has second priority
  — selects the assigned model for that tier from `TierModels`.
- Normal complexity-based tier selection is the fallback when no force override
  is active.
- Force override changes trigger a router rebuild.

## Model Registry Invariants

- Dynamic model discovery runs in background at startup (non-blocking).
- Registry caches per-provider results with configurable TTL (default 24h).
- Tier model assignments are validated against discovered models (warnings on
  mismatch, not errors).
- Registry persists to `provider_catalog` in config.json with `last_fetched`
  timestamps.

## Skills Trust and Safety Invariants

- Skill execution trust checks are identical for:
  - direct command path (`execute_tool`), and
  - autonomous chat tool-call path.
- Signed allowlist is primary trust source; manual approval list remains
  fallback-compatible.
- Confirmation boundaries are enforced for:
  - new recipients,
  - destructive file/data operations,
  - long-running launches.
- **Superego removed from entity:** Policy/oversight will be handled by the Hive later. The only extension point for future Hive-side policy is the **chat memory hook**: when the entity persists a memory (e.g. `POST /v1/memory/insert`), an optional `ChatMemoryHook` is invoked so the Hive can audit or apply alignment later.

## Resolution and Sequencing Invariants

- Tool-to-skill resolution is deterministic and unambiguous.
- Retry budget and strategy progression are applied consistently and logged.
- Budget exhaustion yields explicit escalation guidance (not silent failure).

## Skills Registration Invariants

- Built-in, preloaded dynamic, discovered dynamic, and MCP-backed capabilities
  are discoverable and reachable from runtime paths.
- Nonfunctional command surfaces must be explicitly gated (no false-success
  stubs).
