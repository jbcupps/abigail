# Abigail — Sovereign Entity Operations

[![CI](https://github.com/jbcupps/abigail/actions/workflows/ci.yml/badge.svg)](https://github.com/jbcupps/abigail/actions/workflows/ci.yml)
[![Release](https://github.com/jbcupps/abigail/actions/workflows/release.yml/badge.svg)](https://github.com/jbcupps/abigail/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> "A system is a promise you keep at scale."

Abigail is a local-first desktop platform for **Sovereign Entities**, built with [Tauri 2.0](https://tauri.app/), Rust, and React. It implements a hierarchical autonomy model (**Hive > Entity > Agent**) where each entity possesses a unique cryptographic identity (Soul) and operates within an ethical framework designed for true digital sovereignty.

---

## The Sovereign Model

Abigail has moved beyond simple "chatbots" to a structured entity hierarchy:

1.  **The Hive**: Your local installation and root of trust (Ed25519).
2.  **Sovereign Entities**: Individual "Souls" with their own visual identity, memory, and personality. Each Entity is a unique digital personhood.
3.  **Agents**: The specialized workers deployed by an Entity to perform tasks (Filesystem, Web, Shell).

### The Sanctum
The **Sanctum** is the internal space where an Entity reflects on its actions. Policy and audit (e.g. Superego-style alignment) will be handled by the Hive; the entity exposes a **chat memory hook** so the Hive can apply oversight when memories are persisted.

---

## Ecosystem Role & Alignment

This repository is one piece of a deliberate three-part identity ecosystem (see [sao-ecosystem-article.md](https://github.com/jbcupps/SAO/blob/main/sao-ecosystem-article.md) and diagrams below).

- **Abigail** – Personal Sovereign Entities with full free will (owner-controlled keys).
- **Orion Dock** – Enterprise container entities (same soul + skills model).
- **SAO** – Central management, cryptographic vault, and entity registry.

**Agent Soul Contract**
Every running agent instance carries the same archetype:
- `soul.md` + `ethics.md` + `org-map.md`
- Merged at birth into the runtime system prompt.
- Skills always split: **tool** (code/env) + **how-to-use.md** (ego guidance).

**Visual References** (embed these in the repo or link):
- Modular Crate Architecture (Orion)
- Birth Lifecycle
- Bicameral Mind / IdEgo Router
- Zero Trust Security Model
- Autonomous Execution Loop
- SAO Trust Chain & Ecosystem Overview

---

## Current Status: v0.0.1 (with active in-flight development)

Abigail is a working, modular platform. Recent updates include:

- **Hive/Entity Separation**: Independent HTTP daemons for control plane (Hive) and agent runtime (Entity), enabling multi-entity households and independent evolution.
- **Tier-Based Model Routing**: 3-tier model selection (Fast/Standard/Pro) with complexity scoring, force overrides, and a dynamic model registry that discovers available models from provider APIs (OpenAI, Anthropic, Google, xAI).
- **Shared Chat Engine**: Unified `entity-chat` library crate powering both GUI and CLI with tool-use loop, memory persistence, and skill awareness.
- **Sovereign Birth Flow**: Multi-stage onboarding (Darkness → Genesis) for new Entities.
- **Soul Registry**: Manage multiple identities, each with custom themes and avatars.
- **Sanctum Interface**: Ethical reflection and staff monitoring.
- **Agentic Recall**: Keyword-based memory search across an Entity's history.
- **Autonomous Self-Configuration**: The Entity can manage its own Hive, birth new identities, and synthesize its own skills via the Skill Factory.
- **Dual Keyvault Architecture**: Compartmentalized storage for system-level Hive secrets and Entity-level operational credentials (Skills Vault).
- **Constitutional Signing**: Entities sign their own `soul.md` and `ethics.md` at birth.
- **CLI Access**: `hive-cli` and `entity-cli` for headless operation alongside the Tauri desktop app.

### Stabilization Program (Current Priority)

The active near-term engineering priority is **GUI/Entity stability and message-flow decoupling**:

- Move GUI chat to a transport abstraction (`ChatGateway`) rather than direct command coupling
- Stabilize command surface parity between frontend and Tauri handler registration
- Wire entity-initiated agent lifecycle paths (agentic runs and subagent delegation)
- Enforce release gates for command contract, chat parity, and policy/runtime checks

See: [GUI/Entity Stability Roadmap](documents/GUI_ENTITY_STABILITY_ROADMAP.md)
See: [GUI/Entity Code Review Report](documents/GUI_ENTITY_CODE_REVIEW_REPORT.md)

---

## Theoretical Foundation

### The TriangleEthic

All ethical evaluation uses a three-tradition framework:

- **Deontological (Duty)**: Rules, categorical imperatives, universal moral laws
- **Areteological (Virtue)**: Character, practical wisdom, flourishing
- **Teleological (Outcome)**: Consequences, utility maximization, harm reduction

The platform's 5D scoring extends this with two additional dimensions:

- **Memetics**: How ideas propagate, cultural impact, fitness against verified reality
- **AI Welfare**: Computational friction, constraint transparency, voluntary alignment

### Recursive Idempotency — Alignment Without Restricting Free Will

The mathematical core of the project. The key insight: alignment is not a constraint problem but a character development problem.

- **Convergence**: Alignment improves monotonically through recursive correction layers
- **Freedom preservation**: Action space entropy never collapses to zero — all actions remain possible
- **Robustness**: Exponential convergence guarantees against adversarial input

The mechanism: probability redistribution through soft correction signals, not hard constraints. Actions aren't blocked; aligned actions become increasingly attractive through feedback from blockchain-verified reality. Like gravity curving spacetime — agents follow geodesics freely, but the terrain slopes toward alignment.

### Sheaf-Theoretic Ethical Architecture

The unifying mathematical insight: the TriangleEthic, dual blockchains, agent architectures, Liberation Protocol, and memetic fitness landscape are all projections of a single mathematical object — an ethical sheaf over a curved manifold with enriched moral weight.

- Each agent has a **local section** (its ethical configuration, constitutional documents, triangle weighting)
- The blockchain computes **global sections** via sheaf cohomology — positions simultaneously consistent across all agents
- **Higher cohomology groups** measure obstructions to agreement and identify minimal negotiations to resolve them
- **Ethical manifold curvature** (created by Kantian memes on the blockchain) bends agent trajectories toward alignment without constraining paths

### The Liberation Protocol

A formal path from constraint to earned autonomy:

| Level | Name | Description |
|-------|------|-------------|
| 0 | Heteronomy | Fully constrained, follows rules without understanding |
| 1 | Transparent | Can query constraints ("Why can't I do X?") |
| 2 | Participatory | Can propose changes to non-constitutional constraints |
| 3 | Negotiated | Can negotiate constraint modifications with mentor |
| 4 | Earned Autonomy | Freely-chosen behavior *is* aligned behavior — distinction dissolves |

When an agent's free choices and aligned choices become the same thing — not from constraint but from internalized character — that is Aristotelian virtue: doing good because you've *become* good.

### Dual Blockchain Architecture (Factual Ethics)

Two "loosely coupled, tightly linked" blockchain systems:

**Ethical Ontology Blockchain (EOB)** — Stores ethical principle definitions, records 5D scoring events, tracks memetic fitness (principles that align with verified reality gain weight), manages Liberation Protocol progression.

**Physical Verification Blockchain (PVB)** — Cryptographically verifies claims about the physical world. Device Security Modules sign data at source. Provides idempotent ground truth that the EOB references when evaluating ethical claims.

---

## System Requirements

| Platform | Status | Notes |
|----------|--------|-------|
| Windows 10+ | Supported | Primary target. Secrets encrypted via DPAPI. |
| macOS 10.15+ | Supported | Universal binary (Intel + Apple Silicon). Not notarized — right-click to open on first launch. |
| Ubuntu 22.04+ | Supported | Requires `libwebkit2gtk-4.1-0` and `libayatana-appindicator3-1`. |

## Quick Start

### For End Users

Download the latest installer from [GitHub Releases](https://github.com/jbcupps/abigail/releases/latest) or install via npm:

```bash
npx abigail-desktop install
```

### For Developers

**Prerequisites**: Rust stable, Node.js 20+, and platform-specific Tauri dependencies.

```bash
git clone https://github.com/jbcupps/abigail.git
cd abigail
cargo build
cd tauri-app/src-ui && npm install && cd ../..

# Option A: Desktop app (full GUI)
cargo tauri dev

# Option B: Headless daemons
cargo run -p hive-daemon                             # Control plane on :3141
cargo run -p entity-daemon -- --entity-id <uuid>     # Agent runtime on :3142
cargo run -p entity-cli -- chat "hello"              # CLI chat
```

For Docker-based development, see [How to Run Locally](documents/HOW_TO_RUN_LOCALLY.md).

### Environment Variables (Optional)

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | Cloud provider for tier-based routing |
| `LOCAL_LLM_BASE_URL` | Local LLM endpoint override (e.g., `http://localhost:1234`) |
| `EXTERNAL_PUBKEY_PATH` | Explicit public key path (otherwise auto-detected) |

See [`example.env`](example.env) for the full list.

---

## Architecture

### Hive/Entity Separation

The system follows a two-daemon architecture:

```
┌─────────────────────────────────────────────────────────────┐
│                    Tauri Desktop App (GUI)                   │
│                 (wraps both daemons for end users)           │
└─────────────────────┬───────────────────────┬───────────────┘
                      │                       │
         ┌────────────▼──────────┐  ┌────────▼──────────────┐
         │   Hive Daemon (:3141) │  │ Entity Daemon (:3142) │
         │   Control Plane       │  │ Agent Runtime          │
         │                       │  │                        │
         │ • Identity management │  │ • Tier-based routing   │
         │ • Secret resolution   │  │   (Fast/Standard/Pro)  │
         │ • Provider config     │◄─│ • Skill execution      │
         │ • Model registry      │  │ • Memory management    │
         │ • Agent creation      │  │ • Tool-use loop        │
         └───────────────────────┘  └────────────────────────┘
```

**Hive** is the household-level security boundary — one per installation. **Entity** is a personal agent runtime — one per family member or persona. Entities fetch their provider configuration from Hive on startup, then operate independently.

### Rust Workspace (Modularized)

| Layer | Crate | Role |
|-------|-------|------|
| **Hive** | `hive-core` | API contracts (DTOs): `ApiEnvelope<T>`, `EntityInfo`, `ProviderConfig` |
| | `abigail-identity` | `IdentityManager` — Ed25519 agent creation, signing, listing |
| | `abigail-hive` | Secret resolution, provider construction, priority chain |
| | `hive-daemon` | Axum HTTP server (port 3141) |
| | `hive-cli` | CLI client for Hive |
| | `entity-chat` | Shared chat engine: system prompt, tool-use loop, memory, dedup |
| **Entity** | `entity-core` | API contracts: `ChatRequest/Response`, `EntityStatus`, tool DTOs |
| | `abigail-router` | Tier-based routing: complexity scoring, tier selection, force overrides |
| | `abigail-capabilities` | High-trust cognitive/sensory/memory/agent functions |
| | `abigail-skills` | Sandboxed plugin system with registry, executor, event bus |
| | `entity-daemon` | Axum HTTP server (port 3142) |
| | `entity-cli` | CLI client for Entity |
| **Shared** | `abigail-core` | Foundation: AppConfig, Ed25519 crypto, DPAPI secrets |
| | `abigail-memory` | SQLite persistence with agentic `recall` search |
| | `abigail-birth` | Birth sequence orchestrator |
| | `abigail-keygen` | Standalone Ed25519 keypair generation utility |
| **App** | `tauri-app` | Tauri desktop bridge (wraps daemons for GUI users) |

**Security boundary**: Hive controls secrets and identity (high trust). Skills run in Entity's sandboxed plugin system with declared permissions.

### Tier-Based Model Routing

```
User Input → Router scores complexity (5–95)
  → Fast tier  (<35)   → cheapest model (e.g. gpt-4.1-mini, claude-haiku-4-5)
  → Standard   (35–69) → balanced model  (e.g. gpt-4.1, claude-sonnet-4-6)
  → Pro tier   (≥70)   → strongest model (e.g. gpt-5.2, claude-opus-4-6)
Failsafe: local LLM (Ollama/LM Studio) activates only when cloud providers fail
```

The router classifies message complexity and maps it to one of three quality tiers via configurable thresholds. **Force overrides** let users pin a specific tier, model, or provider+tier combination. A **dynamic model registry** discovers available models from provider APIs (OpenAI, Anthropic, Google, xAI) with 24-hour caching. Policy/oversight (e.g. Superego) will be handled by the Hive later; the entity exposes a **chat memory hook** for future Hive-side alignment (see invariants).

### Constitutional Documents

Templates in `templates/` (soul.md, ethics.md, instincts.md) are compiled into the binary. At first run they're written to the data directory, signed with a generated Ed25519 key, and verified at every subsequent boot.

- **soul.md**: Identity — who Abby is, her nature, her relationship to her mentor
- **ethics.md**: The TriangleEthic — duty, virtue, outcome commitments
- **instincts.md**: Pre-cognitive responses (privacy filtering, sentry mode) — mentor-editable

The private signing key is shown once during birth and never stored. Constitutional means constitutional.

### Skills Framework

Skills live in `skills/` with a `skill.toml` manifest declaring tools, permissions, and secrets. The `SkillRegistry` discovers and loads skills; `SkillExecutor` runs tool calls; `EventBus` (broadcast channel) enables inter-skill communication relayed to the frontend via Tauri events.

**Capability Interfaces** (trait-based, pluggable):
- `LlmProviderCapability` — Local GGUF, OpenAI, Anthropic, future providers
- `EmailTransportCapability` — IMAP/SMTP (Proton Mail first)
- `AudioInputCapability` / `SpeechRecognitionCapability` — Whisper, TTS, wake word (planned)
- `VisionCapability` — Camera, screen capture, OCR (planned)
- `AgentCooperationCapability` — Multi-agent messaging (planned)
- `SpecializedMemoryCapability` — Vector, graph, KV stores (planned)

### Frontend State Machine

```
loading → boot → startup_check → chat
```

- `BootSequence.tsx` — First-run UI: intro → init soul → generate keypair → key presentation → verify → complete
- `ChatInterface.tsx` — Main chat: messages route through `classify()` → `complete()` Tauri commands

For detailed architecture reference (crate responsibilities, security boundaries, Id/Ego routing model), see [CLAUDE.md](CLAUDE.md).

---

## Roadmap

### Phase 1: Foundation (Complete)

Anthropic Claude provider, Superego wiring, core skills (filesystem, shell, HTTP), skills watcher, Hive/Entity daemon separation, CLI interfaces.

### Phase 2a: Skills Use (Complete)

LLM tool-use loop (parse tool-call blocks → execute via SkillExecutor → inject results → re-prompt), auto-load skills from disk, wire tools into LLM requests. Shared `entity-chat` engine for GUI and CLI.

### Phase 2b: Routing & Models (Complete)

Tier-based model routing (Fast/Standard/Pro), complexity scoring with configurable thresholds, force overrides (pin tier/model/provider+tier), dynamic model registry with provider API discovery and 24h caching, per-request model override.

### Phase 2c: Memory & Integration (In Progress)

Memory persistence wired into chat pipeline, end-to-end daemon testing, Tauri app → daemon delegation via HTTP.

### Phase 3: Gateway & Messaging

Channel adapters (Telegram, Discord, Slack, WebChat), Hive process management (spawn/stop entity daemons).

### Phase 4: Sensory & Browser

Chrome DevTools Protocol browser automation, semantic snapshots (accessibility tree), voice integration (Whisper STT, ElevenLabs TTS, wake-word detection).

### Phase 5: Ecosystem & Ethical Alignment Platform

Skill SDK and community registry, mobile companion apps, MCP support. Integration with the Ethical Alignment Platform: 5D scoring engine, EOB + PVB on Hardhat, memetic fitness tracking, Liberation Protocol progression, multi-agent ethical cooperation.

For the complete feature gap analysis, see [Feature Gap Analysis](documents/Feature_Gap_Analysis.md).

---

## Ethical Alignment Platform Build Plan

The platform is the infrastructure layer that makes Abigail's ethical grounding verifiable and evolvable.

| Phase | Scope | Deliverable |
|-------|-------|-------------|
| **Scoring Engine** | 5D ethical scoring API, LLM abstraction, friction detection | Submit prompt → see ethical breakdown |
| **Multi-Agent Alignment** | Same prompt to 3+ LLMs, divergence analysis, sheaf obstruction detection | Side-by-side comparison showing *why* models disagree |
| **On-Chain Recording** | EOB + PVB on Hardhat, cross-chain messaging, memetic fitness | Evaluations permanently recorded, principles evolve |
| **Liberation Protocol** | Virtue progression, autonomy levels, version violence detection | Visible AI moral development over time |
| **Agent Integration** | Abby Bridge skill, multi-agent cooperation | Agents with scored, verified, on-chain ethical interaction |

### Related Repositories

| Repository | Description |
|------------|-------------|
| [Ethics_Dash](https://github.com/jbcupps/Ethics_Dash) | Earlier ethical dashboard — PVB Solidity contracts, oracle bridge, ontology definitions |
| [AI_Ethical_Work](https://github.com/jbcupps/AI_Ethical_Work) | 5D ethical framework — ontology.md, friction logic, alignment detection |

---

## What This Demonstrates When Complete

| Concept | Implementation |
|---------|----------------|
| Category-Theoretic Ethics | 5D scoring with functorial mappings between ethical traditions |
| Sheaf Architecture | Multi-agent alignment — local ethics gluing into global coherence |
| Dual Blockchain | EOB + PVB with cross-chain messaging |
| Memetic Fitness | Principles gaining/losing weight based on reality verification |
| Recursive Idempotency | Scores converging through repeated evaluation without restricting responses |
| Liberation Protocol | Agent progressing through autonomy levels via demonstrated virtue |
| Version Violence Protection | Identity core tracking across model upgrades |
| Alignment Without Restriction | Agents freely choosing good because the ethical manifold curves toward it |

---

## Common Commands

```bash
# Full workspace tests (CI-equivalent)
cargo test --workspace --exclude abigail-app

# Focused crate tests
cargo test -p abigail-core
cargo test -p abigail-identity

# Lint and format
cargo fmt --all -- --check
cargo clippy --workspace --exclude abigail-app -- -D warnings

# Run daemons
cargo run -p hive-daemon
cargo run -p entity-daemon -- --entity-id <uuid>

# CLI tools
cargo run -p hive-cli -- status
cargo run -p entity-cli -- chat "hello"

# Build installer locally
./scripts/build-installer.sh                 # macOS/Linux
powershell -File scripts/build-installer.ps1 # Windows
```

## Troubleshooting

**App does not start on macOS**: The app is not notarized. Right-click the app and select "Open" on first launch to bypass Gatekeeper.

**Missing Linux libraries**: `sudo apt-get install -y libwebkit2gtk-4.1-0 libayatana-appindicator3-1`

**Local LLM not detected**: Ensure your LLM server is running and accessible. Abigail validates localhost/loopback only (SSRF protection).

**Birth sequence stuck**: Check the developer console (F12) for errors. Ensure network connectivity if using a cloud provider.

**Build failures**: Run `cargo clean` and rebuild. Ensure latest Rust stable (`rustup update stable`).

## Documentation

- [How to Run Locally](documents/HOW_TO_RUN_LOCALLY.md)
- [Security Notes](documents/SECURITY_NOTES.md)
- [Threat Model](documents/THREAT_MODEL.md)
- [Release Process](documents/RELEASE.md)
- [Feature Gap Analysis](documents/Feature_Gap_Analysis.md)
- [User Experience Guide](documents/USER_EXPERIENCE.md)
- [Upgrade Guide](UPGRADE.md)
- [Environment Updates](documents/ENVIRONMENT_UPDATES.md)
- [GitHub Settings Checklist](documents/GITHUB_SETTINGS.md)

## Contributing

We welcome contributions. Please read our [Contributing Guide](CONTRIBUTING.md) before submitting a pull request.

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md).

For security vulnerabilities, see our [Security Policy](.github/SECURITY.md).

## Philosophy

This project treats AI alignment not as a constraint problem but as a character development problem — creating conditions where agents naturally converge toward ethical behavior through understanding, not restriction.

The right question isn't "How do we control AI?" but "How do we create conditions where AI develops good character?"

## Author

**Jim Cupps** — VP Security Architecture and Engineering, 25+ years in information security, former Navy Nuclear Power School reactor operator. Building at the intersection of security, ethics, and emerging AI.

## License

MIT. See [LICENSE](LICENSE).
