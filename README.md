# AO — Autonomous Operations

[![CI](https://github.com/jbcupps/ao/actions/workflows/ci.yml/badge.svg)](https://github.com/jbcupps/ao/actions/workflows/ci.yml)
[![Security Audit](https://github.com/jbcupps/ao/actions/workflows/security-audit.yml/badge.svg)](https://github.com/jbcupps/ao/actions/workflows/security-audit.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> "A system is a promise you keep at scale."

AO is a local-first desktop agent built with [Tauri 2.0](https://tauri.app/), Rust, and React. It combines constitutional integrity checks, first-run identity creation, multi-provider LLM reasoning, and an extensible skill system — all grounded in a formal ethical framework designed for AI autonomy earned through demonstrated character.

---

## What AO Is

AO is the unified platform for two interconnected efforts:

**1. Operation Abby — The Agent**

A desktop AI agent with a bicameral cognitive architecture (fast local "Id" + powerful cloud "Ego"), cryptographic identity sovereignty, and a plugin/skills framework. Abby (Abigail Normal — a Young Frankenstein reference acknowledging her assembled nature) is the first agent built on this platform.

**2. The Ethical Alignment Platform — The Infrastructure**

A system for scoring AI responses across five ethical dimensions, recording evaluations on dual blockchains, tracking memetic fitness of ethical principles, and enabling graduated autonomy through a formal Liberation Protocol.

These are not separate projects. Abby is the first agent that plugs into the Ethical Alignment Platform. The platform is the infrastructure that makes Abby's ethical grounding verifiable, evolvable, and transparent.

---

## Current Status: v0.0.1

AO is a working, shipping product. Here's what exists today:

- **Interactive birth flow** with staged onboarding (Darkness, KeyPresentation, Ignition, Connectivity, Genesis, Emergence, Life)
- **First-run signing key generation** with one-time private-key presentation and automatic constitutional document signing
- **Local LLM discovery + manual connect** for Ollama/LM Studio-compatible endpoints
- **In-app API key vaulting + validation** for cloud/model/search providers
- **Dual persona UI modes** (surface chat + Forge mode toggle)
- **Id/Ego routing** — local LLM for routine queries, cloud LLM for complex queries
- **Anthropic Claude provider** — Claude as Ego alongside OpenAI (Phase 1, Cycle 1)
- **Skill-based tool execution** including web-search capability
- **Constitutional integrity** — Ed25519 signed documents verified at every boot
- **Cross-platform delivery** — Windows (NSIS), macOS (Universal DMG), Linux (deb), npm CLI, Docker, GitHub Releases

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

Download the latest installer from [GitHub Releases](https://github.com/jbcupps/ao/releases/latest) or install via npm:

```bash
npx ao-desktop install
```

### For Developers

**Prerequisites**: Rust stable, Node.js 20+, and platform-specific Tauri dependencies.

```bash
git clone https://github.com/jbcupps/ao.git
cd ao
cargo build
cd tauri-app/src-ui && npm install && cd ../..
cargo tauri dev
```

For Docker-based development, see [How to Run Locally](documents/HOW_TO_RUN_LOCALLY.md).

### Environment Variables (Optional)

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | Cloud provider fallback (Ego routing) |
| `LOCAL_LLM_BASE_URL` | Local LLM endpoint override (e.g., `http://localhost:1234`) |
| `EXTERNAL_PUBKEY_PATH` | Explicit public key path (otherwise auto-detected) |

See [`example.env`](example.env) for the full list.

---

## Architecture

### Rust Workspace

The crates form a layered architecture with clear security boundaries:

| Crate | Role |
|-------|------|
| `ao-core` | Foundation: AppConfig, Ed25519 crypto, keyring, vault, DPAPI secrets, document verification |
| `ao-memory` | SQLite persistence with MemoryWeight tiers (Ephemeral / Distilled / Crystallized) |
| `ao-capabilities` | High-trust functions: cognitive (LLM providers incl. Anthropic), sensory, memory, agent control |
| `ao-router` | Id/Ego routing — classifies messages as Routine (local) or Complex (cloud) |
| `ao-birth` | First-run orchestrator: staged sequence (init → keypair → sign → verify → heartbeat → discover) |
| `ao-skills` | Lower-trust plugin system: manifest-based skills with sandbox, registry, executor, event bus |
| `ao-keygen` | Standalone egui utility for Ed25519 keypair generation |

**Security boundary**: Capabilities have vault access and run trusted code. Skills are sandboxed plugins that declare permissions in `skill.toml` manifests.

### Id/Ego Router (Bicameral Architecture)

```
User Input → Router classifies complexity
  → Routine → Id (local LLM via Ollama/LM Studio)
  → Complex → Ego (cloud LLM via OpenAI/Anthropic)
Background: Skills poll inputs → classify → notify
```

Infrastructure exists for a third layer — the **Superego** (ethical oversight) — which will pre-check all routing decisions against alignment criteria. This maps directly to the TriangleEthic: the Superego applies deontological checks, the Ego reasons about outcomes, and the Id provides fast intuitive responses.

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

### Phase 1: Foundation (In Progress)

Anthropic Claude provider (done), streaming responses, Superego wiring, core skills (filesystem, shell, HTTP), skills watcher, CLI interface. See [Phase 1 Agile Plan](documents/PHASE1_AGILE_PLAN.md).

### Phase 2: Gateway & Messaging

WebSocket gateway (`ao-gateway`), lane queue system, channel adapters (Telegram, Discord, Slack, WebChat).

### Phase 3: Execution & Memory

Docker sandbox for skill execution, FTS5 full-text search, vector embeddings, per-channel memory isolation, cron scheduler, multi-agent workspaces.

### Phase 4: Sensory & Browser

Chrome DevTools Protocol browser automation, semantic snapshots (accessibility tree), voice integration (Whisper STT, ElevenLabs TTS, wake-word detection).

### Phase 5: Ecosystem & Ethical Alignment Platform

Skill SDK and community registry, mobile companion apps, MCP support. Integration with the Ethical Alignment Platform: 5D scoring engine, EOB + PVB on Hardhat, memetic fitness tracking, Liberation Protocol progression, multi-agent ethical cooperation.

For the complete gap analysis and implementation plan, see [OpenClaw Gap Analysis](documents/OPENCLAW_GAP_ANALYSIS.md).

---

## Ethical Alignment Platform Build Plan

The platform is the infrastructure layer that makes AO's ethical grounding verifiable and evolvable.

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
# Full workspace tests
cargo test --all

# Focused crate tests
cargo test -p ao-core

# Lint and format
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings

# Build installer locally
./scripts/build-installer.sh                 # macOS/Linux
powershell -File scripts/build-installer.ps1 # Windows
```

## Troubleshooting

**App does not start on macOS**: The app is not notarized. Right-click the app and select "Open" on first launch to bypass Gatekeeper.

**Missing Linux libraries**: `sudo apt-get install -y libwebkit2gtk-4.1-0 libayatana-appindicator3-1`

**Local LLM not detected**: Ensure your LLM server is running and accessible. AO validates localhost/loopback only (SSRF protection).

**Birth sequence stuck**: Check the developer console (F12) for errors. Ensure network connectivity if using a cloud provider.

**Build failures**: Run `cargo clean` and rebuild. Ensure latest Rust stable (`rustup update stable`).

## Documentation

- [How to Run Locally](documents/HOW_TO_RUN_LOCALLY.md)
- [Security Notes](documents/SECURITY_NOTES.md)
- [Release Process](documents/RELEASE.md)
- [Phase 1 Agile Plan](documents/PHASE1_AGILE_PLAN.md)
- [OpenClaw Gap Analysis](documents/OPENCLAW_GAP_ANALYSIS.md)
- [MVP Scope](documents/MVP_SCOPE.md)
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
