# OrionII Alignment Plan — Bus-First Backend, Bicameral Pipeline, Sub-Agents

Status: proposal (2026-06-09)
Reference architecture: `E:\repo\OrionII` (bus spine + Id/Ego/Superego subscribers + SAO seam)
Scope: Abigail backend (`crates/`), both daemons, skills/connectors/providers, birth & entity management.

---

## 1. Where the two architectures stand today

OrionII's core insight is **"the event bus is the entity"**: every cognitive layer (Id, Ego,
Superego, governance, egress, journal) is a subscriber on a typed bus; UI commands are thin
publishers; every envelope carries a `soul_ref` (hash of the charter) and a `correlation_id`
that traces one user keystroke through the whole pipeline.

Abigail already owns analogs of almost every OrionII part — but they are wired as a
request/response service with the bus as a side-channel, not as the spine:

| OrionII | Abigail today | Gap |
|---|---|---|
| `EventBus` trait, `Topic` enum, `Envelope{soul_ref, correlation_id}` (`bus/mod.rs`) | `StreamBroker` trait + `MemoryBroker`/`IggyBroker` (`abigail-streaming`), ad-hoc string topics (`chat-topic`, `job-events`, `conversation-turns`, `conscience-check`, `ethical-signals`) | No typed topic set, no envelope contract, no soul_ref, correlation only inside `ExecutionTrace` |
| Id subscriber: personality consult → curated system prompt → `IdReaction` (`id.rs`, `curator.rs`) | Id = fallback LLM provider in `IdEgoRouter`; preprompt enrichment is keyword inference in `MentorChatMonitor` | **Biggest gap.** Id never *colors* the Ego prompt; birth artifacts (MentorProfile, Triangle Ethic, soul.md) are not load-bearing at runtime |
| Ego subscriber consumes `IdReaction`, publishes `EgoAction` (`ego.rs`) | `route_unified()` called inline from the Axum chat handler (`entity-daemon/routes.rs`) | Chat handler does the work itself; bus observers only watch |
| Superego subscriber on `EgoAction`, soul_ref recorded (`superego_local.rs`) | `ConscienceConsumer` pattern-matching stub; verdicts logged, never gate anything | Same maturity (both stubs) but Abigail's isn't on the response path |
| Sub-agents: roadmap only (M3) | `SubagentManager` + SurrealDB `JobQueue` + `subagent_runner` + topic-grouped results | **Abigail is ahead** — needs bus integration and per-subagent providers, not invention |
| Bus transports: InMemory / NATS JetStream sidecar / Iggy | MemoryBroker / Iggy skeleton | Comparable; do NOT copy the NATS sidecar (heavy for a family desktop) |
| Birth: idempotent `GET /birth` re-read every launch; hot-swap core on config apply | entity-daemon fetches provider-config from hive-daemon at startup; birth wizard is hive-side | Hive-side edits don't fully propagate; no hot-swap |
| Charter + blake3 soul_ref; signed BirthCertificate from SAO | Ed25519-signed soul docs + constitution (stronger than OrionII's surrogate!) | Signatures exist but nothing references them per-message |
| SAO = external governance over HTTP seam | hive-daemon = local control plane over HTTP seam | Architecturally equivalent; hive-daemon plays SAO's role locally. Keep it local-first. |

Conclusion: this is a **rewiring project, not a rewrite**. The crates stay; the chat path,
topic contract, and the Id/Superego roles change.

---

## 2. Phase 1 — Typed bus contract on top of StreamBroker

New module `abigail-streaming::bus` (or thin crate `abigail-bus`):

```rust
pub enum Topic {
    MentorInput,         // chat/UI input → Id stage
    IdReaction,          // Id's curated prompt + personality signal → Ego
    EgoDeliberation,     // Ego reasoning/trace tap (audit)
    EgoAction,           // Ego's committed response → UI, Superego, journal, outbox
    SuperegoEvaluation,  // conscience verdicts
    JobEvents,           // queue/sub-agent lifecycle (absorbs "job-events")
    SkillExecuted,       // tool/skill execution audit (new)
    MemoryArchive,       // absorbs "conversation-turns"
    HiveOutbound,        // outbox → hive-daemon sync (egress seam)
    GovernanceInbound,   // hive → entity: config/assignment/policy refresh
}

pub struct Envelope {
    pub topic: Topic,
    pub entity_id: String,
    pub occurred_at: DateTime<Utc>,
    pub soul_ref: String,          // hash of the entity's signed soul/constitution docs
    pub correlation_id: Uuid,      // = turn_id; reuse ExecutionTrace's turn id
    pub payload: serde_json::Value,
}
```

- Topics are an enum; `Topic::as_str()` maps onto existing StreamBroker stream/topic names so
  MemoryBroker and the Iggy skeleton keep working unchanged.
- `soul_ref` comes from the already-signed birth documents (`birth_memory.json` /
  `constitution.json`) — Abigail can ship the *real* thing OrionII only stubs.
- Rule to adopt from OrionII's AGENTS.md: **all inter-component communication inside
  entity-daemon flows through the bus; adding a topic requires a doc note.** Axum handlers
  become thin publishers.
- Migrate the five existing string topics onto the enum; delete stragglers.

Durability: skip the NATS sidecar. Either finish `IggyBroker`, or (simpler, recommended)
add a `SurrealBroker` that journals envelopes into the existing per-entity SurrealDB before
fanning out via broadcast — one storage engine, restart-replayable, zero sidecar processes.

## 3. Phase 2 — Bicameral chat pipeline as subscribers

Convert the inline chat path in `entity-daemon/routes.rs` into OrionII's staged pipeline:

```
POST /v1/chat(/stream)  →  publish Envelope(MentorInput)  →  return/attach SSE
   Id stage (new `id_stage.rs` subscriber):
     - load IdentityState: MentorProfile + TriangleEthicWeights + soul.md (birth outputs)
     - memory retrieval via abigail-memory 4-layer search (>> OrionII's keyword RAG)
     - optional local-LLM personality consult (Id provider, 30s timeout, degraded fallback)
     - synthesize system prompt = continuity note + personality signal + ethics scaffold + memory context
     - publish IdReaction
   Ego stage (new `ego_stage.rs` subscriber):
     - consume IdReaction → route_unified() with tools/streaming as today
     - publish EgoAction {response, status: success|degraded|error} + EgoDeliberation (ExecutionTrace)
   In parallel on EgoAction:
     - Superego evaluation     - journal/memory archive     - outbox (HiveOutbound)     - SSE/UI delivery
```

Notes:
- Non-streaming `POST /v1/chat` awaits its `correlation_id` on `EgoAction` with a timeout —
  the handler stays thin without breaking the existing API contract.
- This is where birth finally pays off: MentorProfile and Triangle Ethic weights become the
  ethics scaffold and personality signal injected on every turn (OrionII's `curator.rs`
  `EthicsOverlay::scaffold` pattern; Abigail's weights are richer — 4 axes vs 3).
- Keep OrionII's resilience details: per-stage timeouts, "degraded" status surfaced to the UI,
  publish-anyway on lock poisoning.

### Superego upgrade (from logging stub to real participant)

1. Keep fast pattern checks (PII/destructive) synchronous as a **pre-delivery gate** on
   `EgoAction`: `Critical + should_block` holds delivery and publishes a
   `WaitingForConfirmation`-style event (the agentic loop already has this state machine).
2. Add async LLM evaluation: the Id (local) provider judges the exchange against the signed
   constitution; verdicts on `SuperegoEvaluation` with `soul_ref`. Local model keeps this
   private and free.
3. Superego also subscribes to `JobEvents` and `SkillExecuted` — sub-agent output and tool use
   get the same conscience coverage as chat.

## 4. Phase 3 — Sub-agents on the bus

Abigail's queue infrastructure is already ahead of OrionII; align and finish it:

- **Spawn = publish.** `SubagentManager::delegate()` stops being a blocking call from the
  router; spawning publishes a job with the parent's `correlation_id`, `subagent_runner`
  consumes, results land on `JobEvents`. Parent turns can await or fire-and-forget
  (significance scoring already decides SilentLog / SpawnAgentic / FlagMentor).
- **Implement `SubagentProvider::Custom`** via hive: hive-daemon exposes *named provider
  profiles* (`GET /v1/providers/profiles/{name}`), so a research sub-agent can run on
  Perplexity while Ego is Claude — this is the multi-provider advantage made concrete.
- **Capability-scoped tools:** intersect `SubagentDefinition.capabilities` with the
  SkillRegistry to build the tool list per sub-agent (today validation checks capabilities
  but tools are passed by the caller).
- **Trace inheritance:** child `ExecutionTrace` carries `parent_correlation_id` and depth;
  enforce a depth limit (2) in the ExecutionGovernor.
- Delete the deprecated `abigail-router::orchestration` scheduler — the queue won.

## 5. Phase 4 — Skills, connectors, capabilities

- **`SkillExecuted` audit topic:** every `SkillExecutor::execute()` publishes an envelope
  (skill id, tool, redacted params, outcome). Memory and Superego observe; the UI gets a
  real activity feed for free.
- **Wire MCP:** `McpServerDefinition` exists in `AppConfig` but is unwired. Implement an MCP
  client adapter that registers each MCP server's tools as a dynamic skill (the
  `DynamicApiSkill` path already exists). This turns the entire MCP ecosystem into Abigail
  connectors with the existing permission model — by far the highest-leverage connector work.
- **Live skill assignment:** hive→entity assignment changes publish on `GovernanceInbound`
  so the SkillRegistry re-provisions without an entity-daemon restart.
- **Sanitized egress seam:** the outbox sync to hive-daemon is Abigail's egress; adopt
  OrionII's rule that *only* the outbox subscriber ships data out of entity-daemon, and add
  key-fragment redaction (`secret|token|key|password`) before records leave — the per-entity
  scoping rule in CLAUDE.md, enforced at the seam.
- Drop legacy email transport remnants entirely (Browser-skill fallback is already policy).

## 6. Phase 5 — Provider capabilities for multiple entities

- **Per-role provider config.** OrionII configures Id and Ego models (and temperatures)
  independently. Extend the hive provider-config payload to
  `{ id: RoleConfig, ego: RoleConfig, superego: RoleConfig, subagent_profiles: {...} }`,
  each with provider/model/temperature (optional — reasoning models reject custom temps).
  `Hive::build_providers` already returns Id+Ego separately; this formalizes it and lets one
  entity run local-Id + Claude-Ego while another runs local-Id + Gemini-Ego.
- **Live provider health.** Port OrionII's `ModelCallStatus { role, provider, state:
  Healthy|Fallback|Degraded }`: the router records the last N call statuses; expose in
  `GET /v1/status` and the Hive cockpit. ExecutionTrace covers one turn; this covers "is my
  Ego healthy *right now*."
- **Hot-swap on provider change.** When hive updates an entity's provider config, publish
  `GovernanceInbound{kind: "provider.refresh"}`; entity-daemon rebuilds `BuiltProviders` and
  swaps the router `Arc` (SubagentManager::update_router already anticipates this). No
  restart, mirrors OrionII's `apply_bundle_config` hot-swap.
- Deduplicate `EgoProvider` (router) vs `ProviderKind` (hive-core) into one enum in a shared
  crate while touching this code.

## 7. Phase 6 — Birth & management streamlining

- **Idempotent birth re-read.** Add `GET /v1/entities/{id}/birth` on hive-daemon returning
  the full runtime identity in one shot: provider config (all roles), skill assignments,
  policy, personality seed (MentorProfile + Triangle Ethic), soul_ref, and a
  `BirthCertificate`. entity-daemon calls it on *every* launch (replacing today's
  provider-config-only fetch), so any hive-side edit takes effect on next start with no
  rebundling — OrionII's best operational property.
- **BirthCertificate record.** Abigail already signs soul docs with master + agent Ed25519
  keys; formalize `{ entity_id, entity_public_key, master_public_key, soul_hash, issued_at,
  signatures }` as the artifact `soul_ref` points to, persisted in hive storage and verified
  by entity-daemon at boot (the verify hooks exist in `abigail-core`).
- **Collapse the wizard's default path.** QuickStart should be one screen — name + template +
  provider — completing in under a minute; Ethics (Soul Forge) and deep Soul Crystallization
  become *re-runnable rites* on a living entity rather than birth blockers. The
  `CrystallizationEngine` doesn't care when it runs; letting families deepen an entity later
  removes the biggest onboarding friction.
- **Entity lifecycle on the bus.** Hive-initiated rename/retire/provider-change events flow
  over `GovernanceInbound` instead of requiring entity-daemon restarts. `Abigail Hive`
  remains undeletable and is the only writer of these events.

## 8. What NOT to copy from OrionII

- **NATS sidecar process** — operationally heavy for a family desktop; SurrealDB-journaled
  broker gives durability with zero new processes.
- **External SAO dependency** — hive-daemon already plays that role locally; privacy and
  local-first are non-negotiable.
- **JSON-file persistence** — Abigail's SurrealDB + 4-layer memory is strictly better.
- **Keyword RAG curator** — use `abigail-memory` vector search in the Id stage instead.

## 9. Suggested sequencing

| Step | Work | Size |
|---|---|---|
| 1 | Typed Topic/Envelope + migrate 5 existing topics + soul_ref helper | S |
| 2 | Id stage subscriber (birth artifacts → system prompt) + Ego stage subscriber; thin chat handlers | M–L |
| 3 | Superego gate + LLM evaluation on local Id provider | M |
| 4 | Sub-agents: bus spawn, custom providers via hive profiles, trace inheritance; delete orchestration module | M |
| 5 | SkillExecuted topic + MCP adapter + live assignments | M (MCP is the big win) |
| 6 | Per-role provider config + health board + hot-swap | M |
| 7 | Birth endpoint + BirthCertificate + one-screen QuickStart | M |
| 8 | SurrealBroker durability (optional, after the contract is stable) | M |

Steps 1–2 are the keystone: once the bus is the spine, every later step is "add a subscriber."
