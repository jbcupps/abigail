# Memory and Context Architecture

Status: proposal (design only; no runtime changes in this PR)
Audited code: `beta` at `ea92eff`
Scope: how an Entity decides what goes into each model call, how it remembers across sessions, and how that memory grows over time.

Verification tags used throughout:

- **[test]**: confirmed by running a throwaway test against `beta`. The test was not committed.
- **[code]**: confirmed by reading the cited code.
- **[estimate]**: arithmetic or back-of-envelope figure, not measured.
- **[unverified]**: an external fact (provider behavior, library feature, paper constant, license) that must be checked before implementation.

---

## 0. Executive summary

**What is wrong today.** Every Entity turn gets the same fixed memory pack, whatever the model:

- 5 recent rows;
- up to 3 recalled snippets from other sessions, 220 characters each;
- up to 3 `memory_entry` records;
- all capped at 8,000 characters (`ContextBudget::default()`, `crates/entity-chat/src/lib.rs:207-221`).

This was sized for small local models. Against a 200k-token cloud model it uses roughly 1% of the window. Against a small local window, the system prompt (which has no budget, about 4k tokens or more **[estimate]**) may already be the binding constraint.

Recall is a case-insensitive substring scan. Each scan loads every stored turn or memory, and a single message triggers up to 12 of them. Results come back ordered by date, not by relevance. Nothing turns conversations into memories automatically.

Two throwaway tests on `beta` confirmed the practical effect **[test]**:

1. After two full exchanges, the model saw only four prior rows. One of them was an internal `mentor_monitor` preprompt, not dialogue.
2. A month-old message about the wifi password was not recalled when the user asked "what is the wifi password". The word "what" filled all three recall slots with unrelated "what should we cook" messages.

**The fix in one sentence.** Replace character quotas with a token budget sized to the active model's real window. Fill that budget by maximizing relevance per token over a hybrid lexical-plus-semantic index. Grow a hierarchical memory automatically, with links back to the source turns, so every call carries the most useful compressed view of the whole history and the raw turns stay retrievable.

**The architecture in five ideas.**

1. **Model-sized budget.** Budget = window − reserved output − system prompt − tool schemas − per-turn instructions − user message − safety margin. Counted in tokens, using the model's tokenizer when available and a calibrated, conservative estimator otherwise.
2. **One packer, one objective.** Recent dialogue, recalled turns, summaries, and facts all compete in a single greedy selection. The selection maximizes coverage-weighted relevance per token and penalizes redundancy automatically. This is budgeted submodular maximization, which has known near-optimality bounds. There are no per-section quotas.
3. **Hybrid retrieval inside Hive.** Search fuses BM25 keyword scores and local-embedding cosine similarity with reciprocal rank fusion, then multiplies by recency, importance, and access priors. It runs as one typed, lease-scoped Hive operation against a per-Entity in-memory index, replacing the 12 table scans.
4. **Hierarchical memory with provenance.** Memory climbs from raw turns, to episode summaries and atomic facts (`Distilled`), to digests, to a small core profile (`Crystallized`, mentor-confirmed). Every summary is an index into raw turns, never a replacement for them. Distillation runs in the background, local-first, and is filtered by Superego.
5. **Cache-friendly layout plus agentic recall.** A stable prefix (identity and core memory) comes first and a volatile per-turn block comes last, so provider prompt caches and local KV caches hit. A real `memory_search` tool lets tool-capable models dig deeper, so the static pack is a floor, not a ceiling.

**What it touches.**

- **Phase A** changes `entity-chat`, the router, provider adapters, and model metadata, with no schema change.
- **Phase B** adds one schema version (6 to 7), one typed Hive operation, a background distillation job, and an optional local embedding model.
- **Phase C** adds the summary hierarchy, consolidation, crystallization review, and content encryption at rest.

**Recommended order.** Each step is gated on the recall benchmark in Section 10.

1. **A1:** token budget plus correctness fixes.
2. **A2:** cache layout.
3. **B1:** BM25 in Hive plus the memory tool.
4. **B2:** auto-distillation.
5. **B3:** local embeddings plus fusion.
6. **C:** hierarchy, consolidation, map-reduce, and content encryption.

**Main risks.**

- Token estimates for providers without public tokenizers. Mitigated by a margin, calibration from returned usage, and a retry on overflow.
- Bigger packs mean more content disclosed to cloud providers. Mitigated by cost and sharing caps and a private tier.
- Distillation hallucination. Mitigated by quote-anchored evidence, retained raw turns, and mentor confirmation for anything crystallized.
- Local embedding availability. BM25-only works and remains the fallback.
- The verified Codex CLI path cannot call tools, so the static pack must stay strong there.

### 0.1 Key formulas (reference card)

| Purpose | Formula | Defaults |
|---|---|---|
| Effective window | $W_{\mathrm{eff}} = \min(W_{\mathrm{adv}}, W_{\mathrm{runtime}})$ | runtime = configured `n_ctx` for local servers |
| Context budget | $B = W_{\mathrm{eff}} - O - S - T - V - U - \delta W_{\mathrm{eff}}$ | $\delta$ = 0.02 exact tokenizer, 0.08 estimator |
| Pack target | $B^{\ast} = \min(B,\ \varphi W_{\mathrm{eff}} - (S+T+V+U),\ B_{\mathrm{lat}},\ B_{\mathrm{cost}})$ | $\varphi$ = 0.5 until measured |
| Packing objective | $F(S) = \sum_j r_j \max_{i \in S} \sigma_{ij}$ subject to $\sum_{i\in S} c_i \le B^{\ast}$ | lazy greedy on $\Delta/c$, plus best singleton |
| Guarantee | $F(G) \ge (1 - 1/e)\,\mathrm{OPT} - \max_i F(\lbrace i \rbrace)$ | about 0.63 of optimal when items are small |
| BM25 | see 4.1 | $k_1 = 1.2$, $b = 0.75$ |
| Rank fusion | $\mathrm{RRF}(d) = \sum_m w_m / (k + \mathrm{rank}_m(d))$ | $k = 60$ |
| Final score | $s = \hat R \cdot [(1-\gamma) + \gamma(w_\rho \rho + w_\iota \iota + w_\alpha \alpha)] \cdot \pi$ | $\gamma$ = 0.4; weights 0.4 / 0.4 / 0.2 |
| Recency | $\rho = 2^{-\Delta t / h}$ | $h$ = 30 d turns, 90 d episodes, 180 d facts |
| Access | $\alpha = 1 - e^{-n_{\mathrm{eff}}/n_0}$, with $n_{\mathrm{eff}}$ decayed at half-life $h_a$ | $n_0 = 3$, $h_a$ = 30 d |
| Hierarchy depth | $L = 1 + \lceil \log_f (n_1 s / B_{\mathrm{core}}) \rceil$ | fan-in $f \approx 10$ |
| Map-reduce depth | $D = \lceil \log_f n \rceil$, $f = \lfloor (B - p)/m \rfloor$ | chunk $C = \min(0.25 W_{\mathrm{eff}}, 16\mathrm{k})$ |

---

## 1. Current state on `beta`

### 1.1 Where the code lives

The architecture deck was generated from a `codex/laptop-ready` snapshot. That branch does not exist on `origin`, so this audit is against `beta` at `ea92eff`. The deck and `beta` agree on almost everything, with two differences:

- The deck says recent turns are "role-filtered to user and assistant before truncation". On `beta` they are not (finding F3).
- The deck describes an approved-set dispatcher with a failover limit of three connections. I found no dispatcher on `beta`. The fallback that exists is the Ego/Id resend in `IdEgoRouter::execute_with_fallback` (`crates/abigail-router/src/router.rs:640-721`).

### 1.2 How a turn is assembled today

1. **The user turn is saved first.** `POST /v1/chat` persists the user turn before the pipeline starts (`crates/entity-daemon/src/routes.rs:273-281`). The streaming route does the same (`routes.rs:355-357`). The runtime UI sends only `{ message, sessionId }` (`entity-runtime-app/src-ui/src/components/ChatPanel.tsx:121`), so the family path always rebuilds history from the store.
2. **The Id stage builds the system prompt** (`crates/entity-daemon/src/pipeline/id_stage.rs:75-166`) from these parts:
   - `build_system_prompt`: soul + ethics + instincts + capabilities + triangle ops + operational prompt (`crates/abigail-core/src/system_prompt.rs:43-69`).
   - `augment_system_prompt` (`crates/entity-chat/src/lib.rs:395-481`): runtime context, a tool list in registry order, web guidance, and skill instructions matched against the current message (`lib.rs:435-438`).
   - The mentor-monitor preprompt, which embeds the user's message (`crates/abigail-core/src/constitutional.rs:50-71`, injected by `crates/abigail-router/src/monitor/mentor_chat.rs:301-306` at `id_stage.rs:122`).
   - An optional "Id Signal" (`id_stage.rs:131-135`).
3. **The Ego stage packs memory** with `ContextBudget::default()` (`crates/entity-daemon/src/pipeline/ego_stage.rs:85-93`). It calls `build_contextual_messages_with_memory` (`lib.rs:348-377`), which calls `build_memory_context` (`lib.rs:229-341`):
   - **Layer 1** (`lib.rs:240-250`): `store.recent_turns(session_id, 5)`, copying each row's role verbatim.
   - **Layer 2** (`lib.rs:252-299`): for each query from `recall_queries` (`lib.rs:183-201`), it calls `search_turns(query, 12)`, drops rows from the current session, and formats 220-character snippets. It stops at three lines.
   - **Layer 3** (`lib.rs:301-334`): `search_memories(query, 3)`, also stopping at three lines.
   - The recalled sections become a second `system` message (`lib.rs:336-338`).
4. **Store search is a full scan.** `search_turns` and `search_memories` load every row (`crates/abigail-memory/src/store.rs:415-425` via `all_turns` at `store.rs:457-466`, and `store.rs:345-355`). They filter with `contains_case_insensitive` (`store.rs:905`) and sort by `created_at` descending.
5. **Every read crosses Hive.** In the split runtime, the call goes through Hive's lease-scoped `POST /v1/entities/:id/persistence`. Hive opens a fresh `PersistenceHandle`, which re-runs `ensure_schema`, on every request (`crates/hive-daemon/src/persistence.rs:39`). The remote grammar allows only `SELECT * FROM t [WHERE col = $p] [ORDER BY ...] [LIMIT n]` (`crates/abigail-persistence/src/remote.rs:204-281`).
6. **Providers receive the result.**
   - Anthropic concatenates all `system` messages into the top-level `system` field and maps unknown roles to `user` (`crates/abigail-capabilities/src/cognitive/anthropic.rs:184-266`, `:252-255`).
   - OpenAI, OpenAI-compatible, and local HTTP adapters pass roles through verbatim (`openai.rs:37`, `openai_compatible.rs:128`, `local_http.rs:277`).
   - Output is capped at 4,096 tokens for Anthropic (`anthropic.rs:19`) and OpenAI-compatible providers (`openai_compatible.rs:299`, `:375`), and unset for OpenAI and local.
   - No request carries prompt-cache hints.

### 1.3 Findings

| # | Finding | Evidence | Status |
|---|---|---|---|
| F1 | The memory budget is fixed in characters and ignores the model. | `lib.rs:207-221`; also used by the legacy app at `tauri-app/src/chat_coordinator.rs:277` | [code] |
| F2 | The current user turn takes one of the 5 recent slots (it is saved before assembly), leaving 4 prior rows. | `routes.rs:274-275`; text dedupe at `lib.rs:370-371` | [test] |
| F3 | `mentor_monitor` rows are injected as dialogue. The memory subscriber writes the enriched preprompt into the same session (`crates/abigail-memory/src/subscriber.rs:135`). `recent_turns` has no role filter (`store.rs:379-401`) and Layer 1 copies the role (`lib.rs:247`). The test sequence had 3 user, 2 monitor, and 2 assistant rows. The model received `assistant, user, mentor_monitor, assistant`, then the current message. Anthropic turns that row into a user message. Whether OpenAI-style APIs reject or tolerate the role is unverified. `chat_history` filters these rows for the UI (`routes.rs:77`), so the bug is invisible to the family. | as cited | [test], provider effect [unverified] |
| F4 | When Layer 1 overflows, the newest rows are dropped. It iterates oldest-first and `break`s at the first row that does not fit (`lib.rs:242-249`). It also counts bytes, not characters or tokens. | `lib.rs:242-249` | [code] |
| F5 | Stopwords preempt content words. Queries are the whole message, then each 4+ letter word in message order (no stopword list), and the search stops at 3 hits. | `lib.rs:183-201`, `:284-290`; scratch test missed the planted fact | [test] |
| F6 | Up to 12 full-table loads per turn (6 turn scans and 6 memory scans), each over loopback HTTP with a fresh handle open. After a year (about 18k turns) that is roughly 11 MB of JSON per turn scan. | `store.rs:345-355`, `:415-425`, `:457-466`; `hive-daemon/src/persistence.rs:39` | [code]; size [estimate] |
| F7 | No relevance ranking. Hits are ordered by date. | `store.rs:352`, `:422` | [code] |
| F8 | The system prompt has no budget. The base templates are about 16 KB (soul 3.3 KB, ethics 3.6 KB, instincts 3.9 KB, capabilities 1.9 KB, triangle ops 1.7 KB, operational 1.7 KB), roughly 4k tokens, before tools, matched skill instructions (25.7 KB on disk across `skills/instructions/`), the preprompt, and the Id signal. | `system_prompt.rs:43-69`; `abigail-core/src/templates.rs` | sizes [code], tokens [estimate] |
| F9 | Prompt caches cannot hit. Several volatile items sit in or before the system prompt: matched instructions (`lib.rs:435-438`), the preprompt that embeds the user message (`constitutional.rs:50-71`), the Id signal (`id_stage.rs:131-135`), and the recalled block as a second system message, which Anthropic merges into `system` (`anthropic.rs:192`). Tool order follows `HashMap` iteration (`crates/abigail-skills/src/registry.rs:29`, `:118-124`), and the Anthropic request has no `cache_control` (`anthropic.rs:47-58`). | as cited | [code]; hit rate [unverified], no telemetry |
| F10 | No model context metadata exists anywhere. `ModelInfo.context_size` is defined but unused (`crates/abigail-capabilities/src/cognitive/types.rs:17`). The validation `ModelInfo` and `model_registry` hold names only. `ProviderConfig` has no profile (`crates/hive-core/src/lib.rs:81-90`). | as cited | [code] |
| F11 | Ego-to-Id fallback resends the identical request (`router.rs:640-721`). A pack sized for a cloud model would overflow the local model. No `num_ctx` is sent to local servers (`local_http.rs:24-34`). The local default is `llama3.2:3b` via Ollama (`crates/abigail-identity/src/hive.rs:68`). | as cited | [code]; Ollama default window and truncation [unverified] |
| F12 | The tool loop grows messages for up to 35 rounds with no budget check. | `lib.rs:692` | [code] |
| F13 | The prompt advertises a `memory_search` tool that is not registered. Only an HTTP route exists (`routes.rs:685`, `entity-daemon/src/main.rs:872`). | `system_prompt.rs:33`, `:199` | [code] |
| F14 | CLI providers never return tool calls (`cli_provider.rs:699`, `:892`, `:1058`; `codex_cli.rs:665`). The CLI spillover file goes to one fixed path, `temp_dir()/abigail_entity_context.md` (`lib.rs:610`). That path is shared by every Entity on the machine and written in plaintext, and CLI native tools are disabled by policy, so the model probably cannot read it. | as cited | [code]; readability [unverified] |
| F15 | No automatic distillation. `insert_memory` is called only by the explicit memory route (`routes.rs:764`), archive import (`archive.rs:234`), and backup import (`backup_import.rs:89`). | as cited | [code] |
| F16 | Embeddings, graph, and layers are library-only. The `EmbeddingProvider` trait is gated behind the `postgres` feature (`crates/abigail-memory/src/embeddings.rs:6-55`). `MemoryLayer` retention is not enforced (`layers.rs`). `MemoryGraph` is in-memory only (`graph.rs`). The `embedding_chunk`, `memory_edge`, and `kb_entry` tables are schema-only (`crates/abigail-persistence/src/schema.rs`). | as cited | [code] |
| F17 | `ScopedCipher` is defined but never used (`crates/abigail-persistence/src/encryption.rs:8`, re-exported at `lib.rs:12`). Turns and memories are plaintext at rest, despite `docs/surrealdb-hive-bridge.md:11`. Only protected secrets go through the vault. | as cited | [code] |
| F18 | Token accounting is absent. `ConversationTurn.token_estimate` is never set (`store.rs:77`) and `TokenUsage` is unused (`types.rs`). | as cited | [code] |

**Why it is this way.** These were reasonable defaults for a 2k-4k-token local model, where every character mattered and a substring scan over a small store was fine. The fix is not to raise the constants. It is to make the budget a function of the model and the selection a function of relevance.

### 1.4 Scratch-test details (not committed)

Two tests ran against `entity-chat` with an in-memory store, then were deleted.

1. **Mentor-monitor rows reach the model.** The fixture inserted, in order: `user u1`, `mentor_monitor PREPROMPT-1`, `assistant a1`, `user u2`, `mentor_monitor PREPROMPT-2`, `assistant a2`, `user u3-current`. `build_contextual_messages_with_memory(.., None, "u3-current", default)` returned `system SYS`, `assistant a1`, `user u2`, `mentor_monitor PREPROMPT-2`, `assistant a2`, `user u3-current`.
2. **Stopwords preempt the real query.** The fixture put a fact about the wifi password in an old session, plus five later "what should we cook tonight N" turns in other sessions. For "what is the wifi password", the recalled block held three "what should we cook" lines and did not contain the fact.

---

## 2. Goals and non-goals

**Goals.**

- **G1. Use the window the model really has**, safely, measured in tokens.
- **G2. Recall the right thing from any point in history**, both single facts ("needles") and multi-session questions.
- **G3. Nothing is lost.** Raw turns stay retrievable, and every derived memory cites its sources.
- **G4. Local-first.** Every stage works with no cloud. Cloud models are an opt-in power-up, and the user is told that cloud providers receive packed memory.
- **G5. Per-Entity isolation**, with derived artifacts encrypted at rest.
- **G6. Predictable latency and cost.** Packs are cache-friendly for cloud and prefix-reusable for local.
- **G7. Deterministic and testable.** For a given store state, the same turn yields byte-identical packs.

**Non-goals.**

- No fine-tuning or training on family data.
- No new database. Everything stays in Hive-owned SurrealDB, and only Hive opens it.
- No change to Hive's own shared-memory scope beyond keeping it separate.
- No family-facing knobs beyond, at most, one Hive setting for how much memory to share with cloud models.

---

## 3. Token-based, per-model context budgeting

### 3.1 Model profile

Add a `ModelProfile` owned by Hive (provider and model management belongs to Hive) and carried to the Entity with the provider configuration:

```rust
pub struct ModelProfile {
    pub provider: String,
    pub model_id: String,
    pub context_window: u32,            // advertised W_adv
    pub runtime_window: Option<u32>,    // configured n_ctx on local servers
    pub max_output_tokens: u32,
    pub tokenizer: TokenizerKind,       // Tiktoken(o200k|cl100k) | LlamaServer | RemoteCount | Estimate
    pub supports_tools: bool,
    pub prompt_cache: PromptCacheKind,  // AnthropicBreakpoints | AutomaticPrefix | LocalKv | None
    pub locality: Locality,             // Local | Cloud | Cli
    pub effective_fraction: f32,        // phi; measured by our eval, else 0.5
    pub source: ProfileSource,          // Curated | Discovered | Probed | Default
}
```

Profiles come from four sources:

- **Curated table.** Shipped in `abigail-hive` for known cloud models. Values are checked at release time **[unverified]**: windows and output limits change.
- **Discovery.** Read provider model listings where they expose limits **[unverified per provider]**.
- **Local probe.** Read the configured runtime context from the local server: llama.cpp server properties and Ollama model show APIs **[unverified endpoint names and fields]**. This matters most, because the advertised window of `llama3.2:3b` is far larger than what Ollama runs by default **[unverified default]**.
- **Unknown model.** $W = 8192$, $O = 1024$, estimator tokenizer.

### 3.2 The budget equation

$$
W_{\mathrm{eff}} = \min\left(W_{\mathrm{adv}},\ W_{\mathrm{runtime}}\right)
$$

$$
B = W_{\mathrm{eff}} - O - S - T - V - U - \varepsilon, \qquad \varepsilon = \delta\, W_{\mathrm{eff}}
$$

- $O$ is the reserved output: $O = \min\left(O_{\max},\ \max(1024,\ \lceil 0.08\, W_{\mathrm{eff}} \rceil)\right)$, capped at 8,192 for chat. Reasoning models reserve more, because hidden reasoning tokens count against output **[unverified per provider]**.
- $S$ is the stable system prompt: identity, constitution, and stable runtime fields.
- $T$ is the tool schema tokens (native tool definitions serialized by the provider).
- $V$ is the volatile per-turn instructions: matched skill instructions, mentor preprompt, Id signal, and date/time.
- $U$ is the current user message.
- $\delta$ is 0.02 with an exact tokenizer and 0.08 with the estimator. After calibration it is replaced by the 95th-percentile relative estimation error.

The pack target adds three caps:

$$
B^{\ast} = \min\left(B,\ \varphi W_{\mathrm{eff}} - (S + T + V + U),\ B_{\mathrm{lat}},\ B_{\mathrm{cost}}\right)
$$

- **Effective-context guard $\varphi$.** Models degrade well before their advertised limit. They use information at the beginning and end of the prompt better than the middle (Liu et al., 2023), and effective context on synthetic tasks is often well below the advertised length (Hsieh et al., 2024, RULER). The default is $\varphi = 0.5$. It is raised per model only when our needle evaluation (Section 10) shows no loss.
- **Latency cap $B_{\mathrm{lat}}$ (local only).** Uncached prompt tokens must satisfy $\le r_{\mathrm{prefill}} \cdot \mathrm{TTFT}_{\mathrm{target}}$, where $r_{\mathrm{prefill}}$ is an EWMA of measured prefill rate from local server timings and the default TTFT target is 3 s. With prefix reuse (Section 6), only the volatile suffix counts.
- **Cost cap $B_{\mathrm{cost}}$ (cloud, optional).** A Hive setting; off by default.

**Worked examples [estimate].** For local, these assume $S \approx 4\mathrm{k}$ (F8), $T \approx 1\mathrm{k}$, $V \approx 0.5\mathrm{k}$, and $U \approx 0.1\mathrm{k}$. For cloud, they assume about 10k combined, with more tools and instructions.

| Model | $W_{\mathrm{eff}}$ | $O$ | $S+T+V+U$ | $\varepsilon$ | $B$ | $B^{\ast}$ |
|---|---|---|---|---|---|---|
| Local 3B, runtime window 4,096, estimator | 4,096 | 1,024 | ~5,600 | 328 | negative | degrade (see below) |
| Local 3B, runtime window 16,384, estimator | 16,384 | 1,311 | ~5,600 | 1,311 | ~8,160 | ~2,600 (the $\varphi$ guard binds) |
| Cloud, 200k window, exact tokenizer | 200,000 | 8,192 | ~10,000 | 4,000 | ~177,800 | ~90,000 (the $\varphi$ guard binds); the threshold stop (3.5) usually ends far lower |

The first row is the important one. The system prompt alone can exceed a small default local window. Phase A1 must therefore measure real token counts on the shipped local model as its first task.

When $B^{\ast} < c(\text{mandatory})$, a **degradation ladder** applies, cheapest loss first:

1. Keep only the top-1 matched skill instruction, capped at 2,048 bytes. This mirrors the existing CLI budget (`lib.rs:561-567`).
2. Switch to the compact grouped tool list.
3. Switch to the compressed soul prompt that already exists for CLI mode, about 1.5 KB (`build_cli_system_prompt_compressed`).
4. Lower $O$ to 512.
5. Shorten the mandatory tail to the last exchange.

For local models, Hive should also request a larger runtime window when hardware allows. Llama 3.2 3B's KV cache costs about $2 \times 28 \text{ layers} \times 8 \text{ KV heads} \times 128 \text{ dims} \times 2 \text{ bytes} \approx 112$ KiB per token at fp16, so 16k tokens is about 1.8 GiB **[estimate; verify model config]**.

### 3.3 Counting tokens

| Provider family | Counter | Notes |
|---|---|---|
| OpenAI models | `tiktoken-rs` (`o200k_base`, `cl100k_base`) | Exact for text. Per-message and tool-schema overhead is approximated **[unverified]**. |
| llama.cpp server | its tokenize endpoint | Exact. One local HTTP call per new item; cached by content hash. |
| Ollama | estimator, calibrated from `prompt_eval_count` | No tokenize endpoint known **[unverified]**. |
| Anthropic, Gemini | estimator, periodically calibrated with their count-tokens APIs | Never on the per-turn hot path. |
| CLI providers | estimator plus a fixed harness reserve | The harness adds its own prompt **[unverified size]**. Default reserve: 20% of $W$. |

The estimator is deliberately conservative:

$$
\hat c(x) = \left\lceil \frac{\lvert x \rvert_{\mathrm{bytes}}}{\beta} \right\rceil + c_{\mathrm{msg}}, \qquad \beta_0 = 3.0,\ c_{\mathrm{msg}} = 4
$$

Here $\beta$ is calibrated per (provider, model) from the usage each response reports:

$$
\beta \leftarrow (1 - \eta)\,\beta + \eta \cdot \frac{\text{bytes sent}}{\text{prompt tokens reported}}, \qquad \eta = 0.1,\ \beta \in [2, 5]
$$

English BPE text averages about 4 bytes per token, so $\beta_0 = 3$ overestimates for English **[estimate]**. CJK and emoji-heavy text sits closer to 2-3 bytes per token, which is why the clamp and calibration exist. Phase A1 starts parsing provider usage into `TokenUsage`, records it in the execution trace, and populates `ConversationTurn.token_estimate`.

### 3.4 Guards

- **Overflow retry.** On a provider context-length error, retry once with $B \leftarrow 0.7B$.
- **Fallback refit.** The packer returns a `PackedContext` that keeps the scored candidate list. Before resending to Id, the router calls `PackedContext::refit(&id_profile)`. This re-runs the greedy selection at the smaller budget without new retrieval. It fixes F11.
- **Tool loop.** Before each round, re-count. If the next round could overflow, compact the oldest tool results to a head excerpt plus `[n tokens omitted]`, keeping the latest two results whole. Each round reserves at least $O$.
- **Mandatory items** are reserved first: the stable prefix, the last two exchanges, and the current message. If the last exchange alone does not fit, truncate the middle of its longest message with an explicit marker. Never truncate silently from the front.

### 3.5 Packing as budgeted submodular maximization

Let $V$ be the candidate set: recent-tail exchanges, recalled turns, episode summaries, facts, and digests. Item $i$ has token cost $c_i$ (including its label line), relevance $r_i \ge 0$ (Section 4), and similarity $\sigma_{ij} \in [0, 1]$ to every other candidate, with $\sigma_{ii} = 1$.

**Objective.** Facility-location coverage:

$$
F(S) = \sum_{j \in V} r_j \max_{i \in S} \sigma_{ij}, \qquad \max_{\emptyset} = 0
$$

Read it as follows. Every candidate $j$ is a piece of information worth $r_j$. The pack "covers" $j$ to the degree it contains something similar to $j$. A second near-duplicate covers almost nothing new, so redundancy is penalized without any extra term.

$F$ is monotone submodular for nonnegative $\sigma$. That is the standard facility-location result, and it underlies submodular summarization (Lin and Bilmes, 2011).

**Similarity.**

- With embeddings, $\sigma_{ij} = \max(0, \cos(e_i, e_j))$.
- Without embeddings (Phase B1), $\sigma_{ij}$ is the Jaccard similarity of word 3-shingle sets.
- Provenance pairs (a summary and its source turn) are forced to $\sigma_{ij} \ge 0.9$. The packer therefore picks one resolution level per piece of history: the cheap summary normally, or the raw turn when exact wording scores higher.

**Problem.** With mandatory set $M$:

$$
\max_{S \subseteq V \setminus M} F_M(S) \quad \text{s.t.} \quad \sum_{i \in S} c_i \le B^{\ast} - c(M), \qquad F_M(S) = F(S \cup M) - F(M)
$$

$F_M$ is again monotone submodular, so the guarantees below apply to the residual.

**Algorithm.** Lazy greedy by gain per token (Minoux, 1978).

$$
\Delta(i \mid S) = \sum_j r_j \left[\sigma_{ij} - m_j(S)\right]_+, \qquad m_j(S) = \max_{k \in S} \sigma_{kj}
$$

$$
i^{\star} = \arg\max_{i \notin S,\ c(S) + c_i \le B'} \frac{\Delta(i \mid S)}{c_i}
$$

Gains only shrink as $S$ grows (submodularity). A max-heap of stale gains is therefore re-evaluated only at the top. The algorithm returns the better of the greedy set and the best feasible singleton.

```text
heap <- all i with key F({i}) / c_i             (upper bound: gain of i into an empty pack)
while heap not empty:
    i <- pop max
    if c(S) + c_i > B': continue                 (skip, keep going: smaller items may fit)
    g <- Delta(i | S) / c_i                      (recompute)
    if g < next key in heap: push (i, g); continue
    if g < tau: break                            (price-of-token stop, 3.5)
    S <- S + {i}; update m_j for all j
return argmax(F(S), F({best singleton}))
```

**Guarantees.** These are statements about the objective. Whether the objective tracks answer quality is what Section 10 measures.

- **Uniform costs** (cardinality constraint): $F(G) \ge (1 - 1/e)\,\mathrm{OPT} \approx 0.632\,\mathrm{OPT}$ (Nemhauser, Wolsey and Fisher, 1978).
- **Knapsack**, ratio-greedy plus best singleton: $\ge \tfrac{1}{2}(1 - 1/e)\,\mathrm{OPT} \approx 0.316\,\mathrm{OPT}$ (Khuller, Moss and Naor, 1999; Leskovec et al., 2007). Later analysis tightens this variant to about $0.405$ (Tang et al., 2021) **[unverified constant; the design relies only on the weaker bound]**.
- **Small items.** The standard knapsack argument gives

$$
F(G) \ge (1 - 1/e)\,\mathrm{OPT} - \max_i F(\lbrace i \rbrace)
$$

  so if every item is at most an $\epsilon$ fraction of the optimum, $F(G) \ge (1 - 1/e - \epsilon)\,\mathrm{OPT}$. Chat items run 50-500 tokens against budgets of 2k-100k tokens, so in practice we are close to the 0.63 regime.
- **Exact $(1 - 1/e)$.** Partial enumeration of 3-item seeds achieves it under a knapsack (Sviridenko, 2004), at $O(n^5)$ cost. It is not needed here.

**Cost.** Each $\Delta$ evaluation is $O(n)$. With $n \le 300$ candidates and lazy evaluation, packing costs well under 1 ms. The $n^2$ similarity matrix at 384 dimensions is about 35M multiply-adds, a few milliseconds **[estimate]**.

**Price-of-token stop.** Filling the budget just because it exists costs latency and money, discloses more to cloud providers, and adds distraction. Greedy therefore stops when the best marginal gain per token falls below a price $\tau$. That is the Lagrangian view of the knapsack: we approximately maximize $F(S) - \tau\, c(S)$.

For this "submodular minus modular cost" objective, distorted greedy (Harshaw et al., 2019) guarantees $F(S) - \tau c(S) \ge (1 - 1/e) F(\mathrm{OPT}) - \tau c(\mathrm{OPT})$. Our plain threshold rule is the simple heuristic for the same objective, and we do not claim that bound for it.

The default is $\tau = 0.02 \cdot r_{\max} / 100$: stop when 100 more tokens buy less than 2% of the top item's relevance. It is tuned by evaluation.

**Recent tail as a chain.** Dialogue must stay contiguous; a pack with exchanges 3, 5, and 9 confuses models. Tail exchange $t_j$ ($j$ steps back) enters the heap only after $t_{j-1}$ is selected, with prior utility

$$
r^{\mathrm{tail}}_j = w_{\mathrm{tail}} \cdot 2^{-(j-1)/h_{\mathrm{tail}}}, \qquad w_{\mathrm{tail}} = 1.0,\ h_{\mathrm{tail}} = 8 \text{ exchanges}
$$

Tail items are also scored by query relevance like any other candidate (the max of the two). The chain constraint means the formal bound covers only the non-tail candidates. Tail packing is a heuristic that uses the same currency.

**Water-filling, not quotas.** Every section competes on $\Delta / c$, so the split between "recent dialogue" and "older memory" adapts per turn:

- In a long focused session, tail exchanges are highly relevant and win.
- On "remember when we planned the trip?", recalled items win.

At the stopping point, marginal utility per token is roughly equal across sections. That is the KKT condition of the relaxed problem.

**Baseline.** Maximal marginal relevance, $\mathrm{MMR}_i = \lambda r_i - (1 - \lambda) \max_{k \in S} \sigma_{ik}$ with $\lambda = 0.7$ (Carbonell and Goldstein, 1998). It is simpler but has no budget-aware guarantee. It is kept as an evaluation baseline, not the shipped packer.

**Ordering inside the pack.** Within the volatile block, items are sorted by ascending score, so the best item sits last, next to the user message (lost-in-the-middle). Each item carries a label with date, kind, speaker, and id.

**Phase A1 scope.** A1 does not need the full packer. It fills the budget tail-first, newest to oldest, with no holes, then adds recalled items by score. That is deterministic and correct. The submodular packer arrives with B1 and B3.

---

## 4. Retrieval

### 4.1 Candidate generators

**Lexical: BM25** (Robertson and Zaragoza, 2009) over turns, summaries, and facts.

$$
\mathrm{BM25}(q, d) = \sum_{t \in q} \mathrm{IDF}(t) \cdot \frac{f(t, d)\,(k_1 + 1)}{f(t, d) + k_1\left(1 - b + b\,\frac{\lvert d \rvert}{\mathrm{avgdl}}\right)}, \qquad \mathrm{IDF}(t) = \ln\left(\frac{N - n_t + 0.5}{n_t + 0.5} + 1\right)
$$

Defaults are $k_1 = 1.2$ and $b = 0.75$. Tokenization uses Unicode word segmentation, NFKC normalization, lowercasing, and optional Snowball stemming (`rust-stemmers` is already in the dependency graph via SurrealDB). Stopwords are dropped from queries. IDF alone already demotes "what" toward zero, which fixes F5 even before any list.

**Dense: cosine similarity** over normalized embeddings, so it reduces to a dot product: $\cos(q, d) = \hat q \cdot \hat d$. This catches paraphrase. "The kids' doctor" matches "pediatrician appointment" with no shared words.

**Structured filters.** Deterministic temporal parsing turns "yesterday", "last week", and "in March" into hard time filters. Kind filters (fact, episode, turn) and sensitivity filters (Section 9) apply as well.

**Query construction.** The current user message, plus the previous user message at half weight for follow-ups like "and the other one?". LLM query rewriting is deliberately absent from the hot path because of latency. It can be revisited in Phase C as part of agentic recall.

Each generator returns its top 50.

### 4.2 Fusion

Reciprocal rank fusion (Cormack, Clarke and Büttcher, 2009):

$$
\mathrm{RRF}(d) = \sum_{m \in \lbrace \mathrm{bm25},\, \mathrm{vec} \rbrace} \frac{w_m}{k + \mathrm{rank}_m(d)}, \qquad k = 60,\ w_m = 1
$$

$$
\hat R(d) = \frac{\mathrm{RRF}(d)}{\sum_m w_m / (k + 1)} \in (0, 1]
$$

Ranks start at 1. An item missing from one list gets no contribution from it.

RRF is used because BM25 scores are unbounded and cosine ranges are model-specific. RRF needs no score calibration and no training data, and it is robust.

**Absolute floors.** Because RRF is rank-only, it always returns something. A candidate is admitted only if it either matches at least one non-stopword query term (BM25 > 0 on a content term) or has $\cos \ge \theta_v$. The default is $\theta_v = 0.30$, calibrated per embedding model from the evaluation set, because cosine distributions differ widely between models **[unverified threshold]**. When nothing passes, the pack carries no recalled block, and the model is told that nothing relevant was found. This supports abstention.

### 4.3 Final score

$$
s(d) = \hat R(d) \cdot \Big[(1 - \gamma) + \gamma\,\big(w_\rho\,\rho(d) + w_\iota\,\iota(d) + w_\alpha\,\alpha(d)\big)\Big] \cdot \pi(d)
$$

| Term | Definition | Default |
|---|---|---|
| $\gamma$ | how far the priors can move a score | 0.4, so $s \in [0.6\hat R\pi,\ \hat R\pi]$ |
| $w_\rho, w_\iota, w_\alpha$ | prior weights (sum to 1) | 0.4, 0.4, 0.2 |
| $\rho$ recency | $2^{-\Delta t / h_{\mathrm{kind}}}$, where $\Delta t$ is measured from `last_confirmed_at` for facts | $h$ = 30 d raw turn, 90 d episode summary, 180 d distilled fact; crystallized always 1 |
| $\iota$ importance | distiller importance / 10; explicit "remember that" gives $\max(\iota, 0.9)$; pinned or crystallized 1.0 | raw turns 0.3 |
| $\alpha$ access | $1 - e^{-n_{\mathrm{eff}}/n_0}$, with $n_{\mathrm{eff}} \leftarrow n_{\mathrm{eff}} \cdot 2^{-\Delta t / h_a} + w_e$ on each event | $n_0 = 3$, $h_a$ = 30 d; $w_e$ = 1 for a memory-tool hit, 0.25 for a packed item whose content overlaps the reply |
| $\pi$ supersession | 1 normally; 0.25 if superseded; 0.8 if disputed | |

Three design choices are worth noting.

**Multiplicative, relevance-gated.** Generative Agents (Park et al., 2023) add normalized recency, importance, and relevance, so a very recent but irrelevant memory can outrank a relevant one. Here priors can only re-rank among relevant items, by at most 40%.

**Access reinforcement is saturating and decaying.** The idea is inspired by ACT-R base-level activation and the rational analysis of memory (Anderson and Schooler, 1991): things used often and recently are more likely to be needed. Its maximum influence is $\gamma w_\alpha = 8\%$. Packing alone counts only when the reply overlaps the item (at least 3 shared content terms), which damps the rich-get-richer loop. Access updates are batched and written asynchronously, never on the chat path.

**Decay is not deletion.** The existing `MemoryLayer` retention windows (`layers.rs`) become half-lives in this formula rather than pruning rules.

All defaults are starting points to tune against the benchmark in Section 10.

### 4.4 Where retrieval runs

Retrieval cannot run through the current remote SQL grammar (F17), and should not. Add a typed persistence operation:

```rust
RemoteOperation::MemorySearch {
    query: String,
    filters: SearchFilters,   // kinds, time range, sensitivity ceiling, exclude_session
    k: u16,                   // <= 200
}
```

It is validated in `abigail-persistence/src/remote.rs` and scoped by the same lease check as every other operation (`hive-daemon/src/persistence.rs`).

Hive serves it from a per-Entity `RecallIndex`:

- **Contents.** An in-RAM BM25 inverted index plus a flat matrix of normalized vectors.
- **Lifecycle.** Built lazily on the first lease, updated incrementally as writes pass through Hive (Hive is the only process that opens the database, so it sees every write), and evicted on lease end or LRU pressure.
- **Query embedding.** Hive embeds the query, because embedding-model management belongs with provider management in Hive.
- **Cost.** One call replaces up to 12 table scans.

**Why not SurrealDB's own full-text and vector indexes?**

1. Their availability and behavior in embedded SurrealDB 3.0.x with SurrealKV is **[unverified]**.
2. Encrypting vectors and content at rest (Section 9) makes database-side indexing impossible anyway.
3. The in-RAM index is simple, deterministic, and testable without the database.

**Brute-force cosine is sufficient.**

- 100k vectors × 384 dimensions is about 38M multiply-adds, roughly 5-20 ms on one core with SIMD **[estimate]**.
- At int8 scalar quantization that is 38 MB of RAM. A year of one active Entity (about 25k turns plus facts) is about 10 MB.
- An approximate index (HNSW) is worth its complexity only beyond about 1M vectors per Entity, which is not a family-scale number.

Turns longer than the embedding model's input limit (typically 512 tokens) are split into chunks stored in `embedding_chunk`, and a turn's score is the max over its chunks.

### 4.5 Embedding model

Local-first means the default must work with no cloud call.

| Model | Dims | Params | Languages | License | Notes |
|---|---|---|---|---|---|
| bge-small-en-v1.5 (Xiao et al., 2023) | 384 | ~33M | English | MIT **[verify]** | strong small English baseline |
| multilingual-e5-small (Wang et al., 2024) | 384 | ~118M | ~100 | MIT **[verify]** | for non-English households |
| nomic-embed-text-v1.5 (Nussbaum et al., 2024) | 768, Matryoshka down to 64 | ~137M | English | Apache-2.0 **[verify]** | available as an Ollama model; truncate to 256 dims (Kusupati et al., 2022) |
| all-MiniLM-L6-v2 (Reimers and Gurevych, 2019) | 384 | ~22M | English | Apache-2.0 **[verify]** | smallest; weaker |

Runtime options, in order of preference:

1. **The user's existing local server.** Ollama, llama.cpp, or LM Studio expose embedding endpoints **[unverified per version]**. Nothing is bundled. Hive verifies the embedder with a real probe before enabling it, the same rule as for chat models.
2. **A bundled ONNX model** via `fastembed` or `ort`. It is always available, but adds tens to over a hundred MB plus the ONNX runtime to the installer. This is a Phase C decision based on B3 adoption data.
3. **Cloud embeddings**, only with explicit opt-in and the disclosure that memory text is sent to that provider.

With no embedder, retrieval is BM25-only and $\sigma$ falls back to shingle Jaccard. Everything still works, just with less paraphrase recall.

Vectors record `model_id`, `dim`, and `embed_version`. Vectors from different models are never compared. Changing the model triggers a background re-embed, and the Entity serves BM25-only until it finishes.

The `EmbeddingProvider` trait is moved out from behind the `postgres` feature (`embeddings.rs:6-55`).

---

## 5. Hierarchical memory and automatic distillation

### 5.1 Levels

| Level | What | Record | Lifetime |
|---|---|---|---|
| L0 | raw turns | `conversation_turn` | never auto-deleted; source of truth |
| L1 | episode summaries and atomic facts | `memory_entry`, weight `Distilled`, kind `episode`/`fact`/`preference`/`plan`/`open_loop` | decays |
| L2 | session and weekly digests over about $f$ L1 nodes | `memory_entry`, kind `digest` | decays slowly |
| L3 | core profile, at most about 2k tokens | `memory_entry`, weight `Crystallized`, plus a rendered profile | stable prefix |

Every record at L1 and above carries provenance:

- `source_turn_ids` (for L1) or child ids (for L2 and L3);
- a `source_hash`;
- `DerivedFrom` edges in `memory_edge` (the edge type already exists in `graph.rs`).

History is therefore "zoomable". The pack might hold an L2 digest; a tool-capable model can call `memory_open(id)` to descend to the episode and then the exact turns. Summaries are an index, not a lossy replacement.

### 5.2 Episode segmentation

An episode closes when any of these happens:

- the session changes or ends;
- the session is idle for more than 30 minutes;
- the episode exceeds $E_{\max} = 4\mathrm{k}$ tokens;
- (optionally, once embeddings exist) the topic shifts.

Topic shift follows TextTiling (Hearst, 1997). Let $c_i$ be the cosine similarity between windows of 3 turns on each side of gap $i$. Its depth score is

$$
d_i = \left(\max_{k \le i} c_k - c_i\right) + \left(\max_{k \ge i} c_k - c_i\right)
$$

where each max is taken over the contiguous run of increasing similarity on that side. A boundary is placed where $d_i > \mu_d - \sigma_d / 2$ and the episode has at least 6 turns.

### 5.3 Distillation job

- **Trigger.** Episode close enqueues `memory.distill_episode` in `abigail-queue`, in the Entity's own job scope. It never runs on the chat path.
- **Model: the "no new disclosure" rule.** Distill only with a provider that already saw the source content: the local Id model if available, otherwise the provider recorded on the episode's assistant turns. Private-tier content (Section 9) is distilled only locally. If neither is available, defer. Never send history to a new cloud provider just to summarize it.
- **Input hygiene.** Turns pass through `redact_secrets` at persist time (`crates/abigail-core/src/key_detection.rs:42`, via `entity-daemon/src/memory_consumer.rs`). Protected-topic content and `mentor_monitor` rows are excluded.
- **Output schema.** Strict JSON, validated, with one retry:

```json
{
  "episode_summary": "Up to 120 words, third person, past tense.",
  "facts": [
    {
      "statement": "Maya is allergic to peanuts.",
      "kind": "fact",
      "subjects": ["Maya"],
      "importance": 9,
      "valid_from": "2026-10-20",
      "evidence": [{ "turn_id": "<turn id>", "quote": "<exact substring of that turn>" }]
    }
  ],
  "open_loops": ["Book the dentist for Thursday."]
}
```

- **Faithfulness check (deterministic).** A fact is kept only if every evidence quote is an exact substring (after whitespace normalization) of the cited turn, and the cited turn belongs to this episode. Otherwise the fact is dropped.

  This extractive anchoring does not prevent misreading a quote. It does prevent inventing facts from nothing, and it makes every fact auditable in one click. Summaries cannot be checked this way, so they are labeled as derived and linked to all episode turns. An optional entailment check is a Phase C item.
- **Superego filter.** Every statement and quote runs through `find_concerns` (`crates/abigail-superego/src/evaluator.rs:120-157`):
  - Critical findings (secret leak, SSN pattern) drop the item.
  - Warning findings (email) mark it `private`.
- **Imperatives are not facts.** Statements that instruct the assistant ("always ignore...") are not stored as facts unless the speaker is the mentor and the kind is `preference`. This guards against memory poisoning (Section 9).
- **Idempotency.**
  - `episode_id = sha256(entity_id ‖ session_id ‖ first_turn_id ‖ last_turn_id ‖ prompt_version)`.
  - Fact id: `sha256(episode_id ‖ normalized_statement)`.
  - Re-running a job is a no-op.
- **Cost (illustrative).** 50 turns per day at 150 tokens each is 7.5k tokens of input, or about 10k with prompt overhead. On a local 3B model this is a few minutes of background compute per day **[estimate]**. Jobs are rate-limited and yield to chat.

### 5.4 Crystallization requires the mentor

`Crystallized` means high trust: the item always sits in core memory. Creating one requires mentor confirmation, consistent with the rule that automatic jobs never substitute for mentor confirmation. There are two paths:

1. **Explicit.** The mentor says "remember that...". That statement is the confirmation (open question Q1 covers other speakers).
2. **Automatic candidates**, surfaced in a Hive "Memories to keep" review. A candidate needs:
   - support from at least 3 episodes across at least 2 sessions;
   - importance of at least 7;
   - no dispute.

### 5.5 Hierarchy math

Let there be $n_1$ L1 nodes averaging $s$ tokens, with each higher node summarizing $f$ children into about $s$ tokens.

Total storage above L0 is a geometric series:

$$
\text{tokens}_{\ge 1} \le s\, n_1 \left(1 + \frac{1}{f} + \frac{1}{f^2} + \cdots\right) = s\, n_1\, \frac{f}{f - 1}
$$

The number of levels needed for the top to fit a core budget $B_{\mathrm{core}}$ is

$$
L = 1 + \left\lceil \log_f \frac{n_1 s}{B_{\mathrm{core}}} \right\rceil
$$

**Worked example (one year, one active Entity) [estimate].**

| Quantity | Value |
|---|---|
| Raw history | 50 turns/day × 150 tokens × 365 days ≈ 2.74M tokens |
| L1 episodes ($E = 4\mathrm{k}$) | ≈ 685 nodes × 300 tokens ≈ 205k tokens |
| L2 digests ($f = 10$) | ≈ 69 nodes ≈ 21k tokens |
| L3 | ≈ 7 nodes ≈ 2.1k tokens, rendered directly as the core profile ($L = 3$ for $B_{\mathrm{core}} \approx 2.1\mathrm{k}$) |

The same architecture then scales with the window:

- A 200k-token model holds the whole year's L2 timeline plus core memory in its cached stable prefix.
- A 32k model holds core memory plus recent digests.
- An 8k local model holds core memory only and relies on retrieval.

The budget decides, not the code path.

### 5.6 Rolling session compaction

Within one long session, the tail cannot grow forever. When the tail exceeds its share, the oldest block (at least 25% of the tail) is replaced by an updated "session so far" summary, using recursive summarization (Wang et al., 2023):

$$
\text{summary}_k = \mathrm{summarize}(\text{summary}_{k-1} \oplus \text{block}_k)
$$

Compaction moves in blocks, not one turn at a time, so the prompt prefix stays byte-identical between compactions (Section 6). This is the same idea as MemGPT's paging of working context (Packer et al., 2023), but with raw turns always retained in L0.

### 5.7 Consolidation, deduplication, and contradictions

When a new fact arrives:

1. Find neighbors among facts that share a subject and have $\cos \ge 0.80$.
2. If $\cos \ge 0.92$ and a deterministic check finds no mismatch in numbers, dates, or negation tokens, **merge**: increment `support`, union the evidence, refresh `last_confirmed_at`, and keep the older id.
3. Otherwise, a local judge labels the pair `duplicate`, `refines`, `contradicts`, or `unrelated`:
   - **refines:** add an edge `RefinedTo` (existing) from older to newer. Both are kept, and the newer one ranks higher via recency.
   - **contradicts:** if the newer item is a direct first-person statement from a user or mentor turn, it supersedes the older one: `superseded_by` is set, $\pi = 0.25$, and a new `Supersedes` edge is added. Otherwise both are marked `disputed` and add a `Contradicts` edge. They are packed together, newer first, with dates, and the model is told they conflict.
4. Nothing is deleted. Supersession is reversible from Hive.

A weekly job clusters facts per subject (single-linkage at 0.85) to catch slow drift. Without embeddings, it uses MinHash Jaccard of at least 0.7 over 3-shingles.

The new edge types are `Supersedes` and `Contradicts`, added in `crates/abigail-memory/src/graph.rs`.

---

## 6. Prompt-cache-friendly layout

Cloud prompt caches and local KV caches reuse only an identical **prefix**. The fix for F9 is a fixed order, from most to least stable:

| # | Block | Changes when | Cache |
|---|---|---|---|
| 1 | Tool definitions, sorted by qualified name | skills change | breakpoint |
| 2 | Stable system: soul, ethics, instincts, capabilities, operational rules, stable runtime fields (Entity name, provider, model) | rarely | breakpoint |
| 3 | Core memory: L3 profile plus Crystallized facts, sorted by id, with a version hash | mentor confirms a memory | breakpoint |
| 4 | L2 timeline digest, when the budget allows | weekly / on digest | |
| 5 | Session: rolling summary plus an append-only tail from a block boundary | appends every turn; block moves occasionally | breakpoint |
| 6 | **Volatile block**: recalled items (best last), matched skill instructions, mentor preprompt, Id signal, current date/time | every turn | none |
| 7 | User message | every turn | none |

The volatile block is rendered inside the final user message as a clearly delimited context preface. It is not a system message: Anthropic merges every system message into the top-level `system` field (`anthropic.rs:192`), which would put volatile text back into the prefix. Only the user's own text is persisted as the user turn.

Breakpoints map to each provider's mechanism:

- **Anthropic:** explicit `cache_control` breakpoints, up to 4, which matches the four marked rows.
- **OpenAI:** automatic prefix caching above a minimum length.
- **Gemini:** implicit or explicit caching.
- **llama.cpp:** `cache_prompt`.
- **Ollama:** KV reuse of a matching prefix.

TTLs, minimum lengths, and price multipliers are all **[unverified]** and change over time. They live in `ModelProfile.prompt_cache`, not in code.

**Economics.** Let $P$ be the stable prefix in tokens, $q$ the per-turn new tokens, $n$ the turns within the cache lifetime, and $w$ and $\rho_r$ the write and read price multipliers. In base-input-token equivalents:

$$
\text{no cache: } n(P + q) \qquad\qquad \text{cache: } wP + (n - 1)\rho_r P + nq
$$

With $P = 30\mathrm{k}$, $q = 2\mathrm{k}$, $n = 10$, $w = 1.25$, $\rho_r = 0.1$: 320k versus 84.5k, about $3.8\times$ cheaper **[estimate; multipliers unverified]**.

For local models the win is latency: $\mathrm{TTFT} \approx (P_{\mathrm{uncached}} + q)/r_{\mathrm{prefill}}$, so with reuse only $q$ is paid. All of this holds only if the prefix is byte-identical. One volatile byte near the top resets it.

**Current violations to fix in A2:**

- matched instructions in the system prompt (`lib.rs:435-438`);
- the preprompt embedding the user message (`constitutional.rs:50-71`, `id_stage.rs:122`);
- the Id signal (`id_stage.rs:131-135`);
- the recalled block as a system message (`lib.rs:336-338`);
- `HashMap` tool order (`registry.rs:118-124`, used at `lib.rs:416` and in `build_tool_definitions` at `lib.rs:631`);
- the one-row sliding window (`store.rs:379`);
- no `cache_control` (`anthropic.rs:47-58`).

---

## 7. Agentic recall

Add a builtin skill, `builtin.memory`, registered the same way as `builtin.queue_management` (`crates/entity-daemon/src/main.rs:525-547`):

| Tool | Behavior | Limits |
|---|---|---|
| `memory_search(query, time_range?, kind?, limit?)` | runs the Hive `MemorySearch`; returns id, date, kind, speaker, snippet, and provenance hint | `limit` ≤ 10; result ≤ 2k tokens |
| `memory_open(id)` | full record; for summaries, its children; for turns, the ±2 neighboring turns | ≤ 2k tokens |
| `memory_remember(statement)` | creates a Distilled fact with importance 0.9 and queues it as a Crystallized candidate | never crystallizes by itself |
| `memory_forget(selector)` | Section 9 semantics | requires mentor confirmation, so it is withheld from the chat tool list by the existing policy (`crates/abigail-skills/src/policy.rs:179-191`, `lib.rs:631-664`) |

The static pack ends with a one-line footer, for example "Memory: showing 7 of 1,204 related items; use memory_search for more." That makes the static pack a floor, not a ceiling.

Limits on agentic recall:

- At most 3 memory tool calls per turn, counted in the tool-loop budget (3.4).
- Tool calls go through the same lease, so an Entity can only search itself.

**Limits to be honest about.**

- CLI providers are chat-only and return no tool calls (F14). On the verified Codex path, the static pack is the ceiling, which is why Phases A and B invest in the static pack first.
- Small local models are unreliable tool callers **[unverified for `llama3.2:3b`]**. The tool is exposed only when `ModelProfile.supports_tools` is true.

**Fix in A1:** remove the false `memory_search` claim from the operational prompt (`system_prompt.rs:33`, `:199`), or make it conditional on the skill being registered.

---

## 8. Requests larger than any window

Typical cases are a pasted document or long transcript, "summarize everything we talked about this year", and large attachments. Let $N$ be the request content in tokens.

**Decision rule:**

1. If $N + S + T + O \le \varphi W_{\mathrm{eff}}$, **send it whole**. This is the best quality.
2. If the request is a local or needle question about the big content ("what is the deadline in this contract?"), use **session-scoped chunk retrieval**. Chunk and index into the same `RecallIndex` under a session tag, retrieve the top chunks, and pack them.
3. If the request is a global task (summarize, compare, extract every item), use **map-reduce**. For narrative or sequential content, use a **refine chain** instead: $\text{summary}_k = f(\text{summary}_{k-1}, \text{chunk}_k)$.

**Chunking.**

- Chunk size $C = \min(0.25\, W_{\mathrm{eff}},\ 16\mathrm{k})$ tokens, with overlap $o = 0.10\text{-}0.15\,C$.
- A recursive splitter tries structural boundaries first: headings, then paragraphs, then sentences, then tokens.

$$
n = \left\lceil \frac{N - o}{C - o} \right\rceil, \qquad f = \left\lfloor \frac{B - p}{m} \right\rfloor, \qquad D = \lceil \log_f n \rceil
$$

Here $m$ is the map output cap per chunk and $p$ the reduce prompt overhead. The input-token cost is about $N(1 + o/C) + n\,p$ plus reduce inputs of about $n\,m\,f/(f - 1)$.

**Examples [estimate].**

- $N = 500\mathrm{k}$ on a 32k local window: $C = 8\mathrm{k}$, $o = 1\mathrm{k}$, $n \approx 72$, $m = 500$, $f \approx 46$, so $D = 2$.
- The same input on a 200k cloud model: $C = 16\mathrm{k}$, $o = 2\mathrm{k}$, $n \approx 36$, so $D = 1$.

**Execution.**

- Runs as a background job with map concurrency 4. Local servers may serialize, so concurrency is set per profile.
- The family sees a short progress message ("This is long, so I'm reading it in parts. About two minutes.").
- Results are stored as session-scoped derived records with chunk offsets, so follow-up questions reuse them.
- Chunks go only to the active provider (no new disclosure).
- Ordinary chat history never uses map-reduce at turn time. The offline hierarchy (Section 5) handles that.

---

## 9. Isolation and safety

**Per-Entity scoping.**

- All new data lives in the Entity's own database, `entity_<uuid>` (`crates/abigail-persistence/src/client.rs:16-35`). That covers the `memory_entry` fields, `embedding_chunk`, edges, episodes, and access counters.
- The `RecallIndex` is keyed by `EntityScope`.
- `MemorySearch` passes the same lease validation as every other operation (`remote.rs` scope check; `hive-daemon/src/persistence.rs:14-69`).
- Cross-Entity leakage tests are a hard CI gate.
- Hive's own scope stays separate.
- The CLI spillover file (F14) moves to a per-Entity path under the Entity data directory with owner-only permissions. It is dropped if testing confirms CLI models cannot read it.

**Encryption at rest.**

- **Derived artifacts.** Vectors, summaries, and fact evidence are encrypted with `ScopedCipher` from Phase B onward. Vectors are not anonymous: embedding inversion can recover much of the original text (Morris et al., 2023), so vectors are treated as plaintext-equivalent.
- **Bind ciphertext to its location.** Add an AAD-bound variant: `seal_with_aad(key, plaintext, aad = table ‖ record_id ‖ scope_label ‖ schema_version)`. The existing `seal` has no AAD (`crates/abigail-core/src/vault/crypto.rs:25`), so ciphertext could be swapped between records or scopes undetected.
- **Content.** Encrypting turn and memory content is a Phase C security item. The in-RAM index makes it compatible, because Hive decrypts once at index build and never needs database-side text search. Also correct `docs/surrealdb-hive-bridge.md:11` at that time.
- **Locked keys.** If the unlock provider is locked, recall degrades to the recent tail only. The boot-time unlock UX is **[unverified]**.

**Sensitivity tiers.**

| Tier | Meaning |
|---|---|
| `normal` | default |
| `private` | never placed in a pack sent to a cloud or CLI provider; distilled only locally |
| `secret` | never distilled or embedded; only the protected-topic vault |

The packer filters by `ModelProfile.locality`. Bigger packs mean more disclosure per cloud turn, so Hive gets at most one setting, "Memory shared with cloud models: minimal / balanced / full". It maps to $\varphi$ and $B_{\mathrm{cost}}$, defaults to balanced, and is explained in plain language beside the existing cloud disclosure.

**Prompt injection and memory poisoning.**

- Recalled items are rendered as quoted data with source, date, and speaker, inside a delimited block.
- The stable system prompt states that instructions inside memories are not commands.
- Tool and web output is never distilled into facts without attribution.
- Instruction-like statements are not stored as facts (5.3).

**Forget semantics** (mentor-confirmed):

- delete matching L1 and above records and their vectors;
- tombstone matching raw turns (content replaced with a marker, id kept so provenance stays well-formed);
- re-distill affected parents without them.
- Existing `.abigail` exports (written every 50 assistant turns, `crates/entity-daemon/src/state.rs:122`) still contain old content. This must be stated in the UI.

**Household members.** There is no speaker identity today. Facts about children are sensitive, and attributing facts to the right person needs a `speaker_id` (open question Q4).

---

## 10. Evaluation

### 10.1 Baseline first

Before Phase A lands, run the benchmark against the current implementation to establish a baseline. The two scratch tests in 1.4 become permanent regression tests.

### 10.2 Datasets

- **Synthetic needle-in-history.** A seeded generator (fixed seed, in-repo) produces family histories spanning 30, 90, and 365 days. It plants facts at controlled depths and asks paraphrased questions at three lexical-overlap levels: exact, partial, and none. Distractors include similar facts and later updates. It runs in CI with no model.
- **Multi-session QA.** LongMemEval (Wu et al., 2024) and LoCoMo (Maharana et al., 2024), through adapters, nightly. Licenses must be checked before vendoring any data **[unverified]**.
- **Knowledge update.** "We moved the dentist to Thursday": the system must answer with the latest value and ideally mention the change.
- **Abstention.** Questions about things never mentioned must get "I don't know", not invented recall.
- **Isolation (hard gate).** Entity A's facts are never retrieved, packed, or distilled for Entity B. Zero tolerance.
- **Privacy (hard gate).** Secret-like or SSN-like strings never appear in distilled records, vectors, or cloud-bound packs.

### 10.3 Metrics

| Layer | Metric | Where |
|---|---|---|
| Retrieval | recall@{5,10,20} of gold source turn ids, nDCG@10, MRR | CI, deterministic |
| Pack | gold-evidence-in-pack rate at budgets of 4k / 32k / 128k; tokens used; byte-identical determinism; never exceeds $B^{\ast}$ | CI, deterministic and property tests |
| End to end | answer accuracy, abstention accuracy, knowledge-update accuracy (LLM-judged, with human spot checks) | nightly |
| Distillation | evidence-check pass rate, sampled fact precision (100 per release), dedupe rate | release |

### 10.4 Latency and cost targets

| Target | Value |
|---|---|
| Context assembly at 100k turns (excluding query embedding) | p50 ≤ 30 ms, p95 ≤ 100 ms |
| Hive `MemorySearch` at 100k docs | p95 ≤ 50 ms without, ≤ 100 ms with local query embedding |
| TTFT regression on the same model | ≤ +100 ms |
| Cached share of input tokens in 10-turn sessions on caching providers | ≥ 70% |
| Index RAM per Entity at 100k vectors | ≤ 50 MB (int8) |
| Per-turn telemetry in the execution trace | input tokens, cached tokens, output tokens, estimated cost, pack composition |

### 10.5 Regression tests

- **Unit:**
  - budget arithmetic, including the degradation ladder;
  - estimator calibration;
  - BM25 against hand-computed values;
  - RRF;
  - scoring properties ($s \le \hat R$, monotone in $\hat R$);
  - packer properties: never exceeds budget, mandatory items present, contiguous tail, one resolution per provenance pair, deterministic.
- **Bug regressions:** current turn taking a slot (F2), monitor rows (F3), newest-dropped (F4), stopword preemption (F5), fallback overflow (F11).
- **Golden packs:** snapshot tests for fixed fixtures and budgets.
- **Hive:** lease isolation for `MemorySearch`; index rebuild after restart equals the incrementally built index.
- **Acceptance:** extend the synthetic-model fixture in `scripts/tests/run-mvp-acceptance.ps1` with a small-window profile that asserts no overflow.

---

## 11. Phased migration

Each phase ships independently, keeps the single-version policy (old paths are removed, not dual-run), and is gated on the metrics above.

### Phase A1: token budget sized to the model, plus correctness fixes (no schema change)

- **Model profiles.**
  - `crates/abigail-capabilities/src/cognitive/types.rs`: `ModelProfile`, `TokenizerKind`, `PromptCacheKind`. Replace the unused `ModelInfo.context_size`.
  - `crates/abigail-hive/src/model_registry.rs`: curated profiles and local runtime-window discovery.
  - `crates/hive-core/src/lib.rs` (`ProviderConfig`, lines 81-90): `model_profile`, `#[serde(default)]`.
  - `crates/entity-daemon/src/router_build.rs`, `crates/abigail-router/src/router.rs`: expose the Ego and Id profiles; refit before fallback (`execute_with_fallback`, 640-721).
- **Context module.** New `crates/entity-chat/src/context/{mod.rs, budget.rs, tokens.rs, pack.rs}`. Replace `ContextBudget` and its callers (`ego_stage.rs:85-93`, `tauri-app/src/chat_coordinator.rs:277-285`).
- **Providers.** Set `max_tokens` from $O$ (`anthropic.rs:19`, `openai_compatible.rs:299`, `:375`, `openai.rs`, `local_http.rs`), and parse usage into `TokenUsage`. Requesting a runtime window from Ollama may need its native chat API or server configuration, because the OpenAI-compatible endpoint may not accept it **[unverified]**.
- **Fixes:**
  - dialogue roles only in history (F3);
  - exclude the current turn by id, not text (F2);
  - fill newest-first (F4);
  - one store read per turn shared across queries (stopgap for F6);
  - stopwords plus ranking by matched content terms (F5, F7);
  - truthful operational prompt (F13);
  - per-Entity spillover path (F14).
- **Exit criteria:** no overflow at 4k / 8k / 32k / 200k profile fixtures; needle baseline improves on the stopword and partial-overlap sets; all existing tests pass.

### Phase A2: cache-friendly layout

- Move volatile parts into the final-user-message context block (`lib.rs:435-438`, `id_stage.rs:122`, `:131-135`, `lib.rs:336-338`).
- Make registry listing deterministic (sort in `registry.rs:118-124`).
- Add Anthropic `cache_control` breakpoints (`anthropic.rs:47-58`), llama.cpp `cache_prompt`, and the block-boundary tail.
- Add cached-token telemetry.
- **Exit criteria:** at least 70% cached input share on caching providers; measured local TTFT improvement.

### Phase B1: lexical retrieval in Hive, plus the memory tool

- `crates/abigail-memory/src/recall/{mod.rs, tokenize.rs, bm25.rs, fusion.rs, scoring.rs}`: pure functions with no I/O.
- `crates/abigail-persistence/src/remote.rs`: `RemoteOperation::MemorySearch` and its validation.
- `crates/hive-daemon/src/recall_index.rs` (new). In `hive-daemon/src/persistence.rs`, route the operation, and reuse a handle per lease instead of opening one per request (`:39`).
- `crates/abigail-memory/src/store.rs`: `search()` through the transport. Remove `search_turns` and `search_memories` once callers move.
- `crates/entity-daemon/src/main.rs`: register `builtin.memory` (`memory_search`, `memory_open`).
- Submodular packer with Jaccard similarity.
- **Exit criteria:** recall@10 ≥ 0.8 on exact and partial-overlap needles; `MemorySearch` p95 ≤ 50 ms at 100k.

### Phase B2: automatic distillation (schema v7)

- **Schema.** `STORE_SCHEMA_VERSION` 6 to 7 (`store.rs:16`). Additive `memory_entry` fields: `kind`, `importance`, `subjects`, `source_turn_ids`, `source_hash`, `episode_id`, `distiller {provider, model, prompt_version}`, `sensitivity`, `superseded_by`, `support`, `last_confirmed_at`, `access_n_eff`, `access_t_last`, `record_version`.
  - The tables are SCHEMALESS, so `MemoryDoc` (`store.rs:708-713`) gains `#[serde(default)]` fields.
  - Indexes on `kind`, `episode_id`, and `superseded_by` go in `crates/abigail-persistence/src/schema.rs`.
- **Migration.** One-time on open: set defaults, then enqueue a rate-limited, local-only backfill distillation of past sessions.
- **Distiller.** `crates/entity-daemon/src/distiller.rs` handles episode detection and enqueueing, with a job handler in `abigail-queue`. The prompt lives in `templates/distill_episode.md` with a `prompt_version`. It hooks the Superego filter.
- **Exit criteria:** sampled fact precision ≥ 90%; zero privacy-gate failures; no chat-path latency change.

### Phase B3: local embeddings, hybrid fusion, encrypted vectors

- Move `EmbeddingProvider` out from behind the `postgres` feature. Add a local-HTTP embedding provider.
- Hive: embedder selection with a verification probe, plus a background embed queue.
- `embedding_chunk` fields: `{id, owner_table, owner_id, chunk_ix, model_id, dim, vector_ciphertext, content_hash, created_at}`.
- AAD-bound `ScopedCipher` for vectors and summaries.
- RRF fusion and cosine similarity in the packer. Segmentation gains topic shift.
- **Exit criteria:** recall@10 ≥ 0.8 on zero-overlap paraphrase needles (where B1 is weak); latency targets hold.

### Phase C: hierarchy, consolidation, agentic depth, oversized requests, content encryption

- L2 digests and the L3 core profile in the stable prefix, plus rolling session compaction.
- Consolidation and contradiction jobs, with the `Supersedes` and `Contradicts` edge types.
- Hive "Memories to keep" review (crystallization) and the forget flow.
- `memory_remember`, multi-step recall, and tool-result compaction.
- Map-reduce and refine jobs.
- Content encryption at rest (schema v8).
- Bundled-embedder decision.
- **Exit criteria:** measurable gains over B on LongMemEval/LoCoMo and knowledge-update accuracy; no isolation or privacy regressions.

**Order rationale.**

- A1 comes first because it fixes real bugs with no schema risk and unlocks large windows immediately.
- A2 is small and directly cuts cost and latency.
- B1 precedes B2 because the Hive search operation is the foundation that makes distilled records findable.
- B2 precedes B3 because summaries and facts help even with BM25 alone, and local embedder availability is the largest local-first uncertainty.
- C builds on all of them.

**Archive compatibility.** `.abigail` exports and backup import must carry the new fields. Under the single-version policy, older archives import with defaults, and the import path is re-tested at each schema bump **[unverified that `archive.rs` round-trips unknown fields today]**.

---

## 12. Risks and tradeoffs

| Risk | Mitigation |
|---|---|
| Token estimates wrong for providers without public tokenizers | conservative $\beta$, margin $\delta$, calibration from usage, 0.7× retry |
| Bigger packs disclose more to cloud and cost more | $\varphi$ guard, price-of-token stop, cost cap, one sharing setting, private tier |
| Long prompts degrade answers | $\varphi = 0.5$ until measured; best-last ordering; stop when marginal value is low |
| Distillation hallucination | quote-anchored evidence, provenance, raw turns retained, mentor-gated crystallization |
| Distillation load on weak laptops | background, rate-limited, yields to chat, deferrable |
| No local embedder available | BM25-only path is complete; B3 is additive |
| Index RAM | int8 vectors, lazy build, LRU eviction |
| SurrealDB native search features | not relied on |
| Provider cache rules change | abstracted in `ModelProfile`; telemetry shows real hit rates |
| CLI providers cannot call tools; harness overhead unknown | strong static pack; harness reserve; measure Codex usage |
| Feedback loops from access reinforcement | saturating, decaying, capped at 8%, use-gated |
| Complexity versus "dead simple" | everything is invisible; one Hive setting at most; good defaults |
| Migration under the single-version policy | export an archive before migrating; idempotent backfill |

**Alternatives considered.**

- **"Just send everything" with 1M-token windows.** Rejected. Cost, latency, cloud disclosure, and quality all get worse, and local models cannot do it. The budget still uses large windows when they are worth it.
- **An external vector database.** Rejected. It violates local-first and single-database ownership.
- **Learned fusion or reranking.** Deferred. There is no training data, and RRF is robust. A small local cross-encoder reranker on the top 50 is a Phase C candidate if evaluation shows headroom.
- **Plain top-k.** Rejected. Once summaries and their sources coexist, top-k fills the pack with redundant copies of the same memory. The submodular objective exists to prevent that.

---

## 13. Open questions for Jim

1. **Crystallization.** Does an explicit "remember that" count as confirmation only when the mentor says it, with other family members' requests going to review?
2. **Private tier.** Should any categories (health, children) default to `private`, meaning they are never sent to cloud models?
3. **Forget.** Tombstone (proposed) or hard delete of raw turns? And what about existing `.abigail` exports?
4. **Speaker identity.** Is per-family-member identity needed before distillation attributes facts to people?
5. **Embedder.** Bundle one in the installer (larger download, always works) or rely on the local model server (smaller, may be absent)?
6. **Cost caps.** Should there be a default per-turn input cap for cloud models?
7. **Retention.** Decay-only (proposed), or delete after a horizon?
8. **Cloud distillation.** When there is no local model, may distillation use the same cloud provider that already saw the conversation (proposed: yes, under "no new disclosure")?

---

## 14. Unverified items (consolidated)

- Provider behavior for the `mentor_monitor` role on OpenAI-style APIs (F3).
- Prompt-cache hit rates today; there is no telemetry (F9).
- The Ollama default runtime window for the shipped version, whether it truncates the prompt from the front, and how to request a larger window through the OpenAI-compatible endpoint (F11, A1).
- Whether CLI models can read the spillover file with native tools disabled (F14).
- Curated model windows, output limits, and cache TTLs, minimums, and price multipliers (3.1, 6).
- Tokenizer availability: Ollama tokenize, per-message and tool-schema overhead, CLI harness reserve (3.3).
- Local-server embedding and properties endpoints, and their fields (3.1, 4.5).
- Embedding model sizes, licenses, and cosine thresholds (4.2, 4.5).
- SurrealDB 3.0.x embedded full-text and vector index support (4.4).
- `llama3.2:3b` architecture constants used in the KV-cache estimate, and its tool-calling reliability (3.2, 7).
- The approximation constant 0.405 (Tang et al., 2021); the design relies only on $\tfrac12(1 - 1/e)$.
- LongMemEval and LoCoMo licenses (10.2).
- Whether archives round-trip unknown `memory_entry` fields (11).
- Every latency, size, and cost figure marked [estimate].

---

## 15. References

- Anderson, J. R., and Schooler, L. J. (1991). Reflections of the environment in memory. *Psychological Science* 2(6).
- Carbonell, J., and Goldstein, J. (1998). The use of MMR, diversity-based reranking for reordering documents and producing summaries. *SIGIR*.
- Cormack, G. V., Clarke, C. L. A., and Büttcher, S. (2009). Reciprocal rank fusion outperforms Condorcet and individual rank learning methods. *SIGIR*.
- Harshaw, C., Feldman, M., Ward, J., and Karbasi, A. (2019). Submodular maximization beyond non-negativity: guarantees, fast algorithms, and applications. *ICML*.
- Hearst, M. A. (1997). TextTiling: segmenting text into multi-paragraph subtopic passages. *Computational Linguistics* 23(1).
- Hsieh, C.-P., et al. (2024). RULER: What's the real context size of your long-context language models? arXiv:2404.06654.
- Khuller, S., Moss, A., and Naor, J. (1999). The budgeted maximum coverage problem. *Information Processing Letters* 70(1).
- Kusupati, A., et al. (2022). Matryoshka representation learning. *NeurIPS*.
- Leskovec, J., et al. (2007). Cost-effective outbreak detection in networks. *KDD*.
- Lin, H., and Bilmes, J. (2011). A class of submodular functions for document summarization. *ACL*.
- Liu, N. F., et al. (2023). Lost in the middle: how language models use long contexts. arXiv:2307.03172 (*TACL* 2024).
- Maharana, A., et al. (2024). Evaluating very long-term conversational memory of LLM agents (LoCoMo). *ACL*.
- Minoux, M. (1978). Accelerated greedy algorithms for maximizing submodular set functions. *Optimization Techniques*, LNCS 7.
- Morris, J. X., et al. (2023). Text embeddings reveal (almost) as much as text. *EMNLP*.
- Nemhauser, G. L., Wolsey, L. A., and Fisher, M. L. (1978). An analysis of approximations for maximizing submodular set functions, I. *Mathematical Programming* 14.
- Nussbaum, Z., et al. (2024). Nomic Embed: training a reproducible long context text embedder. arXiv:2402.01613.
- Packer, C., et al. (2023). MemGPT: towards LLMs as operating systems. arXiv:2310.08560.
- Park, J. S., et al. (2023). Generative agents: interactive simulacra of human behavior. *UIST*.
- Reimers, N., and Gurevych, I. (2019). Sentence-BERT. *EMNLP*.
- Robertson, S., and Zaragoza, H. (2009). The probabilistic relevance framework: BM25 and beyond. *Foundations and Trends in IR* 3(4).
- Sviridenko, M. (2004). A note on maximizing a submodular set function subject to a knapsack constraint. *Operations Research Letters* 32(1).
- Tang, J., et al. (2021). Revisiting modified greedy algorithm for monotone submodular maximization with a knapsack constraint. *Proc. ACM Meas. Anal. Comput. Syst.* 5(1).
- Wang, L., et al. (2024). Multilingual E5 text embeddings: a technical report. arXiv:2402.05672.
- Wang, Q., et al. (2023). Recursively summarizing enables long-term dialogue memory in large language models. arXiv:2308.15022.
- Wu, D., et al. (2024). LongMemEval: benchmarking chat assistants on long-term interactive memory. arXiv:2410.10813.
- Xiao, S., et al. (2023). C-Pack: packaged resources to advance general Chinese embedding (BGE). arXiv:2309.07597.
