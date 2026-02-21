# LLM Routing and Skills Sequencing Invariants

This file captures the runtime invariants used to verify routing and capability
execution correctness across chat, birth, forge, CLI, and skills commands.

## Entry Invariants

- All LLM-facing commands (`chat`, `chat_stream`, `birth_chat`, CLI chat endpoint)
  must route through a configured `IdEgoRouter` instance.
- Router rebuilds after config changes must update both:
  - active router state, and
  - subagent manager router reference.

## Routing Invariants

- `id_primary` always targets Id.
- `ego_primary` targets Ego only when Ego client is actually available; otherwise
  it must fallback to Id.
- `council` and `tier_based` use fast-path classification for both tool and
  non-tool requests.
- Provider status reporting must never claim an Ego provider when the client
  failed construction.

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
- Superego L2 capability envelope policy is enforced before tool execution.

## Resolution and Sequencing Invariants

- Tool-to-skill resolution is deterministic and unambiguous.
- Retry budget and strategy progression are applied consistently and logged.
- Budget exhaustion yields explicit escalation guidance (not silent failure).

## Skills Registration Invariants

- Built-in, preloaded dynamic, discovered dynamic, and MCP-backed capabilities
  are discoverable and reachable from runtime paths.
- Nonfunctional command surfaces must be explicitly gated (no false-success
  stubs).
