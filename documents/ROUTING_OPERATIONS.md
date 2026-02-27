# Routing Operations Guide

Operator reference for Abigail's LLM routing system: diagnostics, telemetry interpretation, configuration, and troubleshooting.

## Routing Modes

| Mode | Behavior | When to use |
|------|----------|-------------|
| `tier_based` | Scores message complexity (5–95), maps to Fast/Standard/Pro tier via thresholds | Default; balances cost and quality automatically |
| `ego_primary` | All queries to the cloud provider using the Standard tier model | Testing a single provider, or when complexity scoring adds no value |
| `council` | Multi-provider deliberation (draft → critique → synthesis) | High-stakes decisions requiring diverse model perspectives |

## Selection Reasons

Every routing decision records a `selection_reason` explaining why a particular tier/model was chosen:

| Reason | Meaning |
|--------|---------|
| `complexity` | Tier was selected by scoring the message against `TierThresholds` |
| `pinned_tier` | `ForceOverride.pinned_tier` forced a specific tier |
| `pinned_model` | `ForceOverride.pinned_model` forced an exact model, bypassing all tier logic |
| `setup_intent` | Message matched setup/credential keywords; auto-escalated to Pro |
| `ego_primary` | EgoPrimary mode; no tier selection applies |
| `council` | Council mode; multi-provider deliberation |
| `fallback` | Primary provider failed; response came from fallback provider |

**Override precedence** (highest to lowest):
1. `pinned_model` — exact model ID, bypasses everything
2. `pinned_tier` (+ optional `pinned_provider`) — forces a tier
3. `setup_intent` — auto-detected credential/config intent → Pro
4. `complexity` — normal complexity-score-based tier selection

## Execution Trace

The `ExecutionTrace` is the single source of truth for what actually happened during a routing decision. All UI display and attribution is derived from it.

### Key Fields

| Field | Type | Description |
|-------|------|-------------|
| `turn_id` | UUID | Unique identifier for this conversation turn |
| `routing_mode` | string | Active mode: `tier_based`, `ego_primary`, `council` |
| `configured_provider` | string? | Provider the router was configured to prefer |
| `configured_model` | string? | Model resolved by tier/config before execution |
| `configured_tier` | string? | Tier selected before execution (`fast`/`standard`/`pro`) |
| `complexity_score` | u8? | Score (5–95) from complexity classifier; `None` if not tier-based |
| `selection_reason` | string? | Why this tier/model was selected (see table above) |
| `target_selected` | string | Fast-path classifier result: `ego` or `id` |
| `steps` | array | Ordered list of provider call attempts |
| `final_step_index` | usize | Index into `steps` of the step that produced the final response |
| `fallback_occurred` | bool | `true` if response came from a fallback, not the primary target |

### Derived Values

These are computed from the trace, not stored separately:

- **`final_provider()`** — provider label from `steps[final_step_index]`
- **`final_model()`** — model requested in the final successful step
- **`final_tier()`** — `configured_tier` unless `fallback_occurred` (then `None`)

### Reading a Trace

Example trace from a tier-based request:
```json
{
  "turn_id": "abc-123",
  "routing_mode": "tier_based",
  "configured_provider": "openai",
  "configured_model": "gpt-4.1-mini",
  "configured_tier": "fast",
  "complexity_score": 22,
  "selection_reason": "complexity",
  "target_selected": "ego",
  "steps": [
    {
      "provider_label": "openai",
      "model_requested": "gpt-4.1-mini",
      "result": "success",
      "started_at_utc": "...",
      "ended_at_utc": "..."
    }
  ],
  "final_step_index": 0,
  "fallback_occurred": false
}
```

Example trace with fallback:
```json
{
  "configured_tier": "standard",
  "complexity_score": 55,
  "selection_reason": "complexity",
  "steps": [
    { "provider_label": "openai", "result": "error", "error_summary": "429 rate limit" },
    { "provider_label": "local_http", "result": "success" }
  ],
  "final_step_index": 1,
  "fallback_occurred": true
}
```

When `fallback_occurred` is true, the configured tier is not meaningful since the response came from a different provider. The UI will not display a tier badge in this case.

## Diagnostics

### Diagnose Command (No LLM Call)

Test what the router would do for a given message without making any LLM call:

```bash
# Via entity-cli
cargo run -p entity-cli -- diagnose "hello"
cargo run -p entity-cli -- diagnose "Design a distributed system with CQRS"

# Via HTTP
curl "http://127.0.0.1:3142/v1/routing/diagnose?message=hello"

# Via Tauri (from frontend devtools)
await invoke("diagnose_routing", { message: "hello" })
```

Returns a `RoutingDiagnosis`:
```json
{
  "mode": "tierbased",
  "target": "ego",
  "selected_tier": "fast",
  "selected_model": "gpt-4.1-mini",
  "complexity_score": 18,
  "selection_reason": "complexity",
  "ego_provider": "openai",
  "has_local_llm": false,
  "council_available": false,
  "council_provider_count": 0,
  "force_override_active": false,
  "force_override_detail": null
}
```

### Log-Based Debugging

Enable detailed routing logs:
```bash
RUST_LOG=abigail_router=debug cargo run -p entity-daemon -- ...
```

Key log patterns to look for:
- `Tier selected by complexity score N: Tier` — normal tier-based selection
- `Tier pinned by force override: Tier` — override is active
- `Setup/credential intent detected` — auto-escalation to Pro
- `Council has N provider(s) — degraded passthrough` — council with too few providers
- `Ego provider failed, falling back to Id` — fallback triggered
- `Council deliberation failed, falling back to ego/id` — council failure with fallback

### Chat Interface Indicators

The chat UI shows routing details when the routing-details toggle is enabled:

| Badge | Meaning |
|-------|---------|
| `Fast · model-name` | Normal tier-based selection |
| `pinned` (purple) | Force override is active |
| `setup` (blue) | Setup intent auto-escalation |
| `council` (cyan) | Council mode deliberation |
| `fallback` (yellow) | Response came from fallback provider |
| `score:N` | Complexity score shown alongside tier |

## Forge Configuration

### Control Flow

All routing configuration changes follow the Preview → Apply → Audit workflow:

1. **Select** routing mode, provider, model, overrides in the Forge panel
2. **Preview** shows a diff of what will change, with risk level assessment
3. **Apply** commits changes; high-risk changes require explicit confirmation
4. Changes are logged in the audit trail

### Override Controls

- **Standard Tier Model** — the model used for the Standard tier with the active provider
- **Pinned Model Override** — forces an exact model ID, bypassing all tier logic. Staged via Preview/Apply, not saved immediately.
- **Pinned Tier** — forces a specific tier (Fast/Standard/Pro). Staged via Preview/Apply.

### Complexity Thresholds

Default thresholds (configurable in `config.json`):
- Score < 35 → **Fast** tier
- Score 35–69 → **Standard** tier
- Score ≥ 70 → **Pro** tier

## Troubleshooting

### "Model reported doesn't match what I configured"

1. Run `diagnose_routing` with a test message to see what the router would select
2. Check if a `ForceOverride` is active (`force_override_active` in diagnosis)
3. Check if fallback occurred (response may have come from a different provider)
4. Review `execution_trace` in the chat response for the full step-by-step path

### "Tier routing seems stuck on one tier"

1. Check `force_override` in `config.json` — `pinned_tier` or `pinned_model` will bypass complexity scoring
2. Run `diagnose_routing` with messages of varying complexity to see score distribution
3. Review `tier_thresholds` in config to ensure boundaries are reasonable

### "Council mode isn't doing multi-provider deliberation"

1. Check `council_provider_count` in the diagnosis output — need ≥ 2 providers
2. Verify multiple API keys are configured in the Hive
3. Look for `degraded passthrough` warnings in logs
4. Council falls back to single-provider ego/id path when insufficient providers

### "Provider shows as unconfigured after restart"

1. Check that API keys are stored in the secrets vault (not just env vars)
2. Verify `config.json` has the correct `active_provider_preference`
3. Run `hive-cli status` to check Hive-level provider resolution
