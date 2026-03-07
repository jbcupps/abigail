# Abigail vs OpenClaw: Gap Analysis & Feature Parity Plan

**Date:** 2026-02-06
**Abigail Version:** 0.0.1 (MVP)
**OpenClaw Version:** Latest (145K+ GitHub stars, 900+ contributors)

---

## Executive Summary

Abigail and OpenClaw are both AI agent/assistant platforms, but they target different form factors and maturity levels. **Abigail** is a desktop-native Tauri 2.0 application (Rust + React) focused on constitutional identity, Ed25519 document signing, and an Id/Ego dual-LLM routing architecture. **OpenClaw** is a Node.js/TypeScript CLI-first agent that connects messaging platforms (WhatsApp, Telegram, Discord, Slack, Signal, iMessage, Teams, Matrix, and more) to AI models via a WebSocket gateway, with Docker-sandboxed execution, 100+ skills, voice integration, and browser automation.

This report identifies **37 feature gaps** across 12 categories, estimates effort for each, and provides a phased implementation plan to bring Abigail to feature parity.

### Addendum (2026-03-01)

This document remains the long-horizon parity analysis.  
The active near-term execution plan for stability and architecture cutover is:

- `documents/GUI_ENTITY_STABILITY_ROADMAP.md`
- `documents/GUI_ENTITY_CODE_REVIEW_REPORT.md`

That roadmap prioritizes command-surface reliability, GUI/Entity chat decoupling, and entity-initiated agent lifecycle stability before broader parity expansion.

> **Historical Reference Notice (2026-03-01):** Most detailed "current state" snapshots in this document were captured on 2026-02-06 and are preserved for parity context. For implementation truth, prefer `README.md`, `CLAUDE.md`, and the GUI/Entity stability roadmap.

2026-03-07 Cryptographic excision complete + topic-model secret autonomy implemented (self-sufficient Entity achieved).

### Updated Skills Strategy - 06 Mar 2026

- Browser skill upgraded to a production-grade Playwright runtime with persistent per-Entity browser contexts under `data/identities/[uuid]/browser_profile`.
- High-level browser commands and OAuth-aware flows now provide the universal fallback path for webmail and other session-first integrations.
- Browser mutations remain gated by TriangleEthic preview, while Sanctum exposes browser session management separately from the protected-topic Secrets Vault.

---

## Table of Contents

1. [Project Comparison Overview](#1-project-comparison-overview)
2. [Feature Gap Inventory](#2-feature-gap-inventory)
3. [Detailed Gap Analysis by Category](#3-detailed-gap-analysis-by-category)
4. [Abigail's Unique Strengths (Not in OpenClaw)](#4-aos-unique-strengths-not-in-openclaw)
5. [Architecture Comparison](#5-architecture-comparison)
6. [Phased Implementation Plan](#6-phased-implementation-plan)
7. [Effort Estimates](#7-effort-estimates)
8. [Risk Assessment](#8-risk-assessment)
9. [Recommendations](#9-recommendations)

---

## 1. Project Comparison Overview

| Dimension | Abigail | OpenClaw |
|-----------|----|---------|
| **Runtime** | Tauri 2.0 (Rust) + React/TypeScript | Node.js 22+ (TypeScript CLI) |
| **Form factor** | Desktop app (Sovereign Entities) | CLI + messaging platform integrations |
| **LLM support** | Anthropic, OpenAI, Local (Ollama) | Anthropic, OpenAI, Ollama, Kimi, MiniMax |
| **Messaging** | None (desktop-only chat) | 12+ platforms (WhatsApp, Slack, etc.) |
| **Execution** | In-process (Tauri) | Docker sandbox + host + device nodes |
| **Skills** | Core (FS, Shell, Web, HTTP) | 100+ bundled AgentSkills |
| **Voice** | None | Always-on speech, Voice Wake |
| **Browser** | None | Full browser control |
| **Memory** | SQLite with Agentic Recall | Markdown files + Vector/Graph search |
| **Security** | Ed25519 signing, DPAPI, Superego Audit | Docker isolation, Tool policy |
| **Identity** | Sovereign Birth, Soul Registry, Sanctum | Simple onboarding |
| **Multi-agent** | Hive > Entity > Agent hierarchy | Multi-agent workspaces |
| **Scheduling** | None | Cron-based task scheduling |
| **Mobile** | None | iOS/Android companion apps |
| **Community** | Solo/small team | 900+ contributors, ClawCon meetups |
| **Release paths** | 6 paths: NSIS, DMG, deb, npm, GitHub Release, abigail-keygen | 1 path: `npm install -g openclaw` |
| **CI/CD** | 5-job gate: lint, 3-platform tests, frontend, audit, CodeQL | GitHub Actions (details vary) |

---

## 2. Feature Gap Inventory

### Legend
- **Priority:** P0 (critical for parity), P1 (high value), P2 (medium), P3 (nice-to-have)
- **Effort:** S (< 1 week), M (1-3 weeks), L (3-6 weeks), XL (6+ weeks)

| # | Feature Gap | Priority | Effort | Category | Status |
|---|-------------|----------|--------|----------|--------|
| 1 | Multi-channel messaging (WhatsApp, Telegram, Discord, Slack, etc.) | P0 | XL | Messaging | Pending |
| 2 | WebSocket gateway / control plane | P0 | L | Architecture | Partial (HTTP daemons done; WebSocket/SSE pending) |
| 3 | Docker sandbox for skill/tool execution | P0 | L | Security | Pending |
| 4 | Expanded LLM provider support (Anthropic, XAI, Google) | P0 | M | LLM | Done (Anthropic) |
| 5 | Skills ecosystem (100+ bundled skills) | P0 | XL | Skills | Done (Hive + Factory) |
| 6 | Browser automation (accessibility tree snapshots) | P1 | XL | Browser | Pending |
| 7 | Voice integration (STT, TTS) | P1 | L | Voice | Pending |
| 8 | Lane queue system (serial execution) | P1 | L | Architecture | Pending |
| 9 | Multi-agent workspaces (Registry) | P1 | L | Multi-agent | Done (Hive Ops) |
| 10 | Cron/scheduled task execution | P1 | M | Scheduling | Pending |
| 11 | Persistent memory as Markdown | P1 | M | Memory | Pending |
| 12 | Semantic search (Recall) | P1 | L | Memory | Done |
| 13 | Mobile companion apps | P1 | XL | Mobile | Pending |
| 14 | Device node system | P1 | L | Mobile | Pending |
| 15 | CLI interface (abigail-cli) | P2 | M | Interface | Done |
| 16 | WebChat interface | P2 | M | Interface | Pending |
| 17 | Live Canvas | P2 | L | UI | Pending |
| 18 | Skills watcher | P2 | S | Skills | Done |
| 19 | Skill install gating (Skills Vault) | P2 | M | Skills | Done |
| 20 | Smart home integrations | P2 | M | Skills | Pending |
| 21 | Music/media service integrations | P2 | M | Skills | Pending |
| 22 | Productivity tool integrations | P2 | M | Skills | Pending |
| 23 | Group chat support | P2 | M | Messaging | Pending |
| 24 | DM pairing and allowlists | P2 | M | Security | Pending |
| 25 | Streaming responses | P2 | M | LLM | Done |
| 26 | Model context protocol (MCP) support | P2 | M | LLM | Pending |
| 27 | Agent-to-UI (A2UI) rendering | P2 | L | UI | Pending |
| 28 | Semantic snapshots for web | P2 | M | Browser | Pending |
| 29 | File system operations skill | P2 | S | Skills | Done |
| 30 | Shell command execution skill | P2 | S | Skills | Done |
| 31 | Per-channel memory isolation | P2 | M | Memory | Pending |
| 32 | Legacy 3-way Superego routing track (obsolete) | P2 | M | LLM | Archived (superseded by tier-based routing + Hive-side policy hook) |
| 33 | Community skill marketplace/registry | P3 | L | Skills | Pending |
| 34 | Onboarding daemon (background service) | P3 | M | Architecture | Done (Hive/Entity daemons) |
| 35 | Telemetry and analytics (opt-in) | P3 | M | Operations | Pending |
| 36 | Plugin/extension API for third-party developers | P3 | L | Skills | Pending |
| 37 | Internationalization (i18n) | P3 | M | UI | Pending |

---

## 3. Detailed Gap Analysis by Category

### 3.1 Messaging Platform Integration (Gaps #1, #23, #24)

**OpenClaw's capability:** Connects to 12+ messaging platforms as a unified inbox. Users interact with the agent through WhatsApp, Telegram, Discord, Slack, Signal, iMessage (via BlueBubbles), Microsoft Teams, Matrix, Google Chat, Zalo, and a built-in WebChat. Each channel can be routed to a different agent workspace with isolated memory.

**Abigail's current state:** Desktop-only chat interface embedded in the Tauri app. No external messaging platform integrations. Communication is limited to the local UI.

**What's needed:**
- A messaging adapter layer (trait-based, one adapter per platform)
- Platform-specific API clients (Telegram Bot API, Discord.js equivalent, WhatsApp Cloud API / Baileys, Slack Bolt equivalent)
- Message normalization layer (convert platform-specific formats to unified `Message` type)
- Group chat support with mention-gating (only respond when mentioned)
- DM pairing/allowlists for security
- Per-channel routing configuration

**Abigail advantage to leverage:** Abigail's existing `LlmProvider` trait pattern and skill system can be extended. The Tauri event bus already supports multi-channel event routing internally.

---

### 3.2 Gateway Architecture (Gaps #2, #8, #34)

**OpenClaw's capability:** A WebSocket server (`ws://127.0.0.1:18789`) serves as the sole control plane. It manages sessions, channels, tools, and events. A "Lane Queue System" enforces serial execution by default to prevent race conditions, with explicit opt-in parallelism for safe operations.

**Abigail's current state:** Tauri command-based architecture. Each frontend action maps to a `#[tauri::command]` handler. No WebSocket gateway, no queue system. Execution is synchronous per-request within the Tauri event loop.

**What's needed:**
- WebSocket server (tokio-tungstenite or axum with WebSocket support)
- Session management (connect/disconnect, authentication)
- Channel abstraction (each messaging platform = one channel)
- Lane queue system with serial-by-default execution
- Event routing from gateway to appropriate agent/workspace
- Background daemon mode (headless operation without Tauri UI)

---

### 3.3 Execution Sandbox (Gap #3)

**OpenClaw's capability:** Docker-based sandbox for all tool execution. Three execution contexts: sandbox (Docker container with isolated filesystem), host (gateway process), and device nodes (paired mobile/desktop devices). The Docker sandbox prevents skill code from accessing the host system directly.

**Abigail's current state:** Skills run in-process within the Tauri app. The `Sandbox` struct in `abigail-skills` defines permission types and resource limits, but enforcement is permission-check-based (allowlists), not process-level isolation. There is no container or process sandboxing.

**What's needed:**
- Docker integration (bollard crate for Rust Docker API)
- Container lifecycle management (create, start, stop, remove per execution)
- Volume mounting for controlled filesystem access
- Network policy enforcement at the container level
- Fallback to in-process execution when Docker is unavailable
- Resource limits (CPU, memory, time) enforced at container level

---

### 3.4 LLM Provider Ecosystem (Gaps #4, #25, #26, #32)

**OpenClaw's capability:** Supports Anthropic Claude (recommended), OpenAI, local models via Ollama, and Chinese models (Moonshot AI Kimi, MiniMax). Users bring their own API keys. Streaming responses are standard.

**Abigail's current state (updated 2026-03-01):** Tier-based routing (Fast/Standard/Pro) is active, streaming is implemented across Tauri/daemon paths, and OpenAI-compatible provider support includes Anthropic/xAI/Google integrations via current router/provider wiring. Entity-side 3-way Superego routing is no longer the active direction; policy extension is via Hive-side hooks.

**What's needed:**
- `AnthropicProvider` implementation (using anthropic-sdk or direct HTTP)
- `XaiProvider`, `GoogleProvider` (Gemini API) implementations
- Streaming response support (SSE or chunked transfer → Tauri events)
- MCP (Model Context Protocol) client for tool-use standardization
- Continue hardening provider fallback/diagnostics and align policy enforcement with Hive-side integration points
- Provider health checks and automatic fallback chain

---

### 3.5 Skills Ecosystem (Gaps #5, #18, #19, #20, #21, #22, #29, #30, #33, #36)

**OpenClaw's capability:** 100+ preconfigured AgentSkills across categories: shell commands, filesystem operations, web automation, 50+ third-party service integrations (smart home, productivity, music). Three skill types: bundled, managed, and workspace skills. A skills watcher hot-reloads changes to `SKILL.md`. Community-driven ecosystem with install gating.

**Abigail's current state:** 2 skills total — `skill-web-search` (Tavily, fully working) and `skill-proton-mail` (manifest only, transport layer exists but not integrated). The `SkillRegistry`, `SkillExecutor`, and `EventBus` infrastructure is solid but underutilized.

**What's needed (prioritized skill categories):**

**Core skills (P0):**
- File system operations (read, write, list, search)
- Shell command execution (with sandbox)
- Web browsing/scraping
- HTTP request skill

**Productivity skills (P1):**
- Calendar integration (Google Calendar, Outlook)
- Email (expand beyond Proton Mail — Gmail, Outlook, generic IMAP)
- Note-taking / document management
- Task management (Todoist, Linear, Jira)

**Communication skills (P2):**
- Messaging platform adapters (as skills)
- Notification routing

**Smart home skills (P2):**
- Home Assistant integration
- Philips Hue, SmartThings, etc.

**Infrastructure:**
- Skills watcher for hot-reload
- Managed skill install/uninstall with gating
- Community skill registry (remote catalog + install from URL/registry)
- Skill development SDK and documentation

---

### 3.6 Browser Automation (Gaps #6, #28)

**OpenClaw's capability:** Full browser control via a dedicated Chrome/Chromium instance. Uses "semantic snapshots" — accessibility tree parsing instead of screenshots. A screenshot can be 5MB; a semantic snapshot is under 50KB, dramatically reducing token costs while providing higher precision for the LLM to understand page content.

**Abigail's current state:** No browser automation. The `sensory/vision.rs` module has stubs returning errors.

**What's needed:**
- Chrome DevTools Protocol (CDP) client (chromiumoxide or fantoccini crate)
- Accessibility tree extraction and serialization
- Navigation, clicking, form-filling actions
- Page content extraction (semantic snapshots)
- Tab/window management
- Cookie and session management
- Screenshot capability as fallback

---

### 3.7 Voice Integration (Gap #7)

**OpenClaw's capability:** Always-on speech with Voice Wake (wake-word detection) and Talk Mode on macOS/iOS/Android. ElevenLabs integration for high-quality TTS. Speech-to-text for voice input.

**Abigail's current state:** `sensory/hearing.rs` has stubs returning errors. No voice capability.

**What's needed:**
- Speech-to-text integration (Whisper API or local whisper.cpp)
- Text-to-speech integration (ElevenLabs, Azure TTS, or local TTS)
- Audio input/output via system audio APIs (cpal crate)
- Wake-word detection (Porcupine or similar)
- Voice activity detection (VAD)
- Streaming audio pipeline

---

### 3.8 Multi-Agent Support (Gap #9)

**OpenClaw's capability:** Route different channels to isolated agent instances via workspaces. Each workspace has its own memory directory, skills configuration, and system prompt. Enables running multiple specialized agents from a single installation.

**Abigail's current state:** Single agent with Id/Ego routing. The `agent.rs` module in `abigail-capabilities` has cooperation stubs. No workspace isolation.

**What's needed:**
- Workspace abstraction (isolated config, memory, skills per workspace)
- Agent registry (multiple named agents)
- Channel-to-agent routing rules
- Shared vs. isolated memory boundaries
- Inter-agent communication protocol

---

### 3.9 Memory & Search (Gaps #11, #12, #31)

**OpenClaw's capability:** Memory stored as local Markdown documents that users can manually read and edit. Search uses BM25 (keyword), vector embeddings, and graph search for semantic retrieval. Per-channel memory isolation ensures conversations don't leak across contexts.

**Abigail's current state:** SQLite with 3-tier `MemoryWeight` system (Ephemeral/Distilled/Crystallized). Memories stored as content strings with weight and timestamp. No semantic search, no vector embeddings, no user-editable format.

**What's needed:**
- Markdown-based memory export/import (alongside SQLite)
- Vector embedding generation (via LLM API or local model)
- Vector similarity search (SQLite VSS extension, or qdrant/lancedb)
- BM25 full-text search (SQLite FTS5 extension)
- Graph-based memory relationships
- Per-channel/workspace memory isolation
- Memory pruning and consolidation pipeline

---

### 3.10 Scheduling (Gap #10)

**OpenClaw's capability:** Cron-based scheduled task execution. Agents can be configured to run tasks on schedules (e.g., daily briefings, periodic checks).

**Abigail's current state:** No scheduling capability.

**What's needed:**
- Cron expression parser (cron crate)
- Task scheduler service (tokio background task)
- Scheduled task persistence (survive app restarts)
- Task definition format (what to run, when, with what context)
- UI for viewing and managing scheduled tasks

---

### 3.11 Mobile & Device Nodes (Gaps #13, #14)

**OpenClaw's capability:** macOS menu bar companion app, iOS and Android companion apps that act as "device nodes." Nodes execute local actions on the paired device with permission handling, enabling cross-device agent interactions.

**Abigail's current state:** Desktop-only Tauri app. No mobile presence.

**What's needed:**
- Device node protocol (WebSocket or gRPC for device pairing)
- iOS companion app (Swift/SwiftUI or React Native)
- Android companion app (Kotlin or React Native)
- macOS menu bar app (could use Tauri's tray icon, already has `tray-icon` feature)
- Device capability discovery and permission management
- Push notification integration

---

### 3.12 UI Enhancements (Gaps #15, #16, #17, #27, #37)

**OpenClaw's capability:** CLI interface for headless operation, WebChat browser-based UI, Live Canvas for agent-driven visual workspaces, Agent-to-UI (A2UI) protocol for dynamic rendering.

**Abigail's current state:** Tauri desktop UI only. React frontend with chat and identity panels. No CLI mode, no web interface, no dynamic canvas.

**What's needed:**
- CLI binary (clap-based, no Tauri dependency)
- Web interface (extract React UI as standalone web app)
- Canvas/whiteboard component for visual agent output
- A2UI protocol for agent-driven UI updates
- i18n framework (i18next or similar)

---

## 4. Abigail's Unique Strengths (Not in OpenClaw)

Abigail has several features that OpenClaw lacks. These are differentiators worth preserving:

| Abigail Feature | Description | OpenClaw Equivalent |
|------------|-------------|---------------------|
| **Constitutional identity** | Ed25519-signed soul.md/ethics.md/instincts.md verified at every boot | None — OpenClaw has no cryptographic identity verification |
| **Birth sequence** | 5-stage interactive identity creation (Darkness→Ignition→Connectivity→Genesis→Emergence) | Simple onboarding wizard |
| **External signing keypair** | Private key given to user, never stored — user owns their agent's identity | No equivalent — identity is config-based |
| **DPAPI secret encryption** | Windows-native encryption for API keys and credentials | Docker isolation (different approach) |
| **Id/Ego/Superego routing** | Psychoanalytic model for LLM routing with ethical oversight layer | Simple model selection |
| **Memory weight tiers** | Ephemeral/Distilled/Crystallized importance levels | Flat Markdown files |
| **Identity repair flow** | Recovery from broken state with private key or hard reset | No equivalent |
| **Rust-native security** | Memory safety, SSRF protection, path traversal prevention at the type level | Node.js with Docker isolation |
| **Multi-path distribution** | 6 release paths: NSIS installer, macOS DMG (universal), Linux deb, npm CLI, GitHub Release, abigail-keygen — all automated in CI | Single npm install path |
| **5-job CI quality gate** | Lint, 3-platform tests, frontend build, cargo+npm audit, CodeQL SAST — gated branch protection | Standard CI (varies) |

**Recommendation:** These features should be preserved and enhanced, not replaced, during the parity effort. They represent Abigail's philosophical and architectural differentiation.

---

## 5. Architecture Comparison

### Abigail (Current — Hive/Entity Separation)
```
┌───────────────────────────────────────────────────────┐
│                Tauri Desktop App (GUI)                 │
│            (wraps both daemons for end users)          │
└────────────────────┬──────────────────┬───────────────┘
                     │                  │
        ┌────────────▼──────┐  ┌───────▼───────────────┐
        │  Hive Daemon      │  │  Entity Daemon        │
        │  (:3141)          │  │  (:3142)              │
        │                   │  │                       │
        │ IdentityManager   │  │ IdEgoRouter           │
        │ SecretsVault      │◄─│ SkillRegistry         │
        │ Hive (resolve)    │  │ SkillExecutor         │
        │ ProviderConfig    │  │ EventBus              │
        └───────────────────┘  └───────────────────────┘
```

### OpenClaw
```
┌──────────────────────────────────────────┐
│              Gateway (WebSocket)          │
│         ws://127.0.0.1:18789             │
│  ┌──────────────────────────────────┐    │
│  │        Lane Queue System         │    │
│  │   (serial by default, safe //s)  │    │
│  └──────────────┬───────────────────┘    │
│                 │                         │
│  ┌──────────────┼───────────────────┐    │
│  │           Brain (LLM)            │    │
│  │  Claude │ OpenAI │ Ollama │ etc  │    │
│  └──────────────┬───────────────────┘    │
│                 │                         │
│  ┌──────────────┼───────────────────┐    │
│  │        Agent Runtime             │    │
│  │  100+ Skills │ Browser │ Shell   │    │
│  └──────────────┬───────────────────┘    │
│                 │                         │
├─────────────────┼────────────────────────┤
│                 │                         │
│  ┌──────┐ ┌────┴──┐ ┌───────┐ ┌───────┐ │
│  │Whats │ │Telegr │ │Discord│ │Slack  │ │
│  │App   │ │am     │ │       │ │       │ │
│  └──────┘ └───────┘ └───────┘ └───────┘ │
│  ┌──────┐ ┌───────┐ ┌───────┐ ┌───────┐ │
│  │Signal│ │iMsg   │ │Teams  │ │Matrix │ │
│  └──────┘ └───────┘ └───────┘ └───────┘ │
│                                          │
│  ┌──────────────────────────────────┐    │
│  │  Docker Sandbox │ Device Nodes   │    │
│  └──────────────────────────────────┘    │
│  ┌──────────────────────────────────┐    │
│  │  Memory (Markdown + BM25/Vector) │    │
│  └──────────────────────────────────┘    │
└──────────────────────────────────────────┘
```

### Proposed Abigail Architecture (Post-Parity)
```
┌──────────────────────────────────────────────┐
│                  Abigail Platform                  │
│                                               │
│  ┌──────────────────────────────────────┐     │
│  │        Gateway (WebSocket/gRPC)      │     │
│  │    Lane Queue + Session Management   │     │
│  └──────────────────┬───────────────────┘     │
│                     │                          │
│  ┌──────────────────┼───────────────────┐     │
│  │      abigail-router (Superego/Ego/Id)     │     │
│  │  Anthropic │ OpenAI │ Ollama │ etc   │     │
│  │  + Streaming + MCP                   │     │
│  └──────────────────┬───────────────────┘     │
│                     │                          │
│  ┌──────────────────┼───────────────────┐     │
│  │         abigail-capabilities              │     │
│  │  Cognitive │ Sensory │ Agent │ Memory│     │
│  │  + Voice   │ Browser │ Multi │ Vector│     │
│  └──────────────────┬───────────────────┘     │
│                     │                          │
│  ┌─────────┐ ┌──────┴──────┐ ┌────────────┐  │
│  │abigail-skills│ │ abigail-sandbox  │ │ abigail-memory   │  │
│  │ 50+     │ │ Docker/Wasm │ │ SQLite+FTS5 │  │
│  │ skills  │ │ isolation   │ │ +Vector     │  │
│  └─────────┘ └─────────────┘ └────────────┘  │
│                                               │
│  ┌───────────────────────────────────────┐    │
│  │          Channel Adapters             │    │
│  │  Desktop │ CLI │ Web │ Telegram │     │    │
│  │  Discord │ Slack │ Matrix │ WhatsApp  │    │
│  └───────────────────────────────────────┘    │
│                                               │
│  ┌───────────────────────────────────────┐    │
│  │Constitutional Identity (Ed25519)      │    │
│  │Birth Sequence │ Repair │ Verification │    │
│  └───────────────────────────────────────┘    │
└──────────────────────────────────────────────┘
```

---

## 6. Phased Implementation Plan

### Phase 1: Foundation (Weeks 1-6) [ALMOST DONE]
**Goal:** Core infrastructure for multi-interface, multi-provider operation

| # | Task | Effort | Dependencies | Status |
|---|------|--------|-------------|--------|
| 1.1 | **Add Anthropic LLM provider** — Implement `AnthropicProvider` with streaming | M | None | Done |
| 1.2 | **Add streaming response support** — SSE/chunked → Tauri events + generic stream trait | M | None | Done |
| 1.3 | **Wire 3-way Superego routing** — Connect existing `TrinityConfig` infrastructure | M | 1.1 | Done |
| 1.4 | **Add core skills: filesystem, shell, HTTP** — Essential tool capabilities | M | None | Done |
| 1.5 | **Implement skills watcher** — Hot-reload skills on file change (notify crate) | S | None | Done |
| 1.6 | **Add CLI interface** — clap-based binary sharing abigail-router/abigail-skills/abigail-memory | M | None | Partial |

### Phase 2: Gateway & Messaging (Weeks 7-14)
**Goal:** Multi-channel communication and gateway architecture

| # | Task | Effort | Dependencies |
|---|------|--------|-------------|
| 2.1 | **Build WebSocket gateway** — New `abigail-gateway` crate (tokio-tungstenite/axum) | L | Phase 1 |
| 2.2 | **Implement lane queue system** — Serial-by-default execution with opt-in parallelism | L | 2.1 |
| 2.3 | **Channel adapter trait** — Abstract messaging platform interface | M | 2.1 |
| 2.4 | **Telegram adapter** — First messaging platform (Bot API, well-documented) | M | 2.3 |
| 2.5 | **Discord adapter** — Second platform (serenity crate) | M | 2.3 |
| 2.6 | **WebChat adapter** — Browser-based chat (extract/extend React UI) | M | 2.1 |
| 2.7 | **Slack adapter** — Third platform (Slack Bolt equivalent) | M | 2.3 |

### Phase 3: Execution & Memory (Weeks 15-22)
**Goal:** Sandboxed execution, advanced memory, and scheduling

| # | Task | Effort | Dependencies |
|---|------|--------|-------------|
| 3.1 | **Docker sandbox integration** — `abigail-sandbox` crate using bollard | L | Phase 2 |
| 3.2 | **Enhanced memory: FTS5 full-text search** — SQLite FTS5 extension | M | None |
| 3.3 | **Enhanced memory: vector search** — Embedding generation + similarity | L | 1.1 |
| 3.4 | **Markdown memory export/import** — User-editable memory files | M | None |
| 3.5 | **Per-channel memory isolation** — Workspace-scoped memory stores | M | 2.3 |
| 3.6 | **Cron scheduler** — Background task scheduling service | M | 2.1 |
| 3.7 | **Multi-agent workspaces** — Isolated agent instances with routing | L | 2.1, 3.5 |

### Phase 4: Sensory & Browser (Weeks 23-30)
**Goal:** Browser automation and voice capabilities

| # | Task | Effort | Dependencies |
|---|------|--------|-------------|
| 4.1 | **Browser automation** — CDP client (chromiumoxide), navigation, interaction | XL | Phase 3 |
| 4.2 | **Semantic snapshots** — Accessibility tree extraction and serialization | M | 4.1 |
| 4.3 | **Voice: speech-to-text** — Whisper integration (API or local whisper.cpp) | L | None |
| 4.4 | **Voice: text-to-speech** — ElevenLabs/Azure TTS integration | M | None |
| 4.5 | **Voice: wake-word detection** — Porcupine or equivalent | M | 4.3 |
| 4.6 | **Additional messaging adapters** — WhatsApp, Signal, Matrix, Teams | L | 2.3 |

### Phase 5: Ecosystem & Mobile (Weeks 31-40)
**Goal:** Skill marketplace, mobile apps, and community features

| # | Task | Effort | Dependencies |
|---|------|--------|-------------|
| 5.1 | **Skill development SDK** — Templates, docs, testing framework | L | Phase 4 |
| 5.2 | **Community skill registry** — Remote catalog, install, update | L | 5.1 |
| 5.3 | **30+ additional skills** — Productivity, smart home, media, etc. | XL | 5.1 |
| 5.4 | **macOS menu bar companion** — Tray icon → lightweight always-on presence | M | 2.1 |
| 5.5 | **Mobile companion app** — React Native or native iOS/Android | XL | 2.1 |
| 5.6 | **Device node protocol** — Cross-device pairing and action execution | L | 5.5 |
| 5.7 | **Live Canvas / A2UI** — Agent-driven visual workspace | L | 2.6 |
| 5.8 | **MCP client support** — Model Context Protocol for tool standardization | M | Phase 1 |

---

## 7. Effort Estimates

### Summary by Phase

| Phase | Duration | New Crates | Key Deliverables |
|-------|----------|-----------|-----------------|
| **Phase 1: Foundation** | 6 weeks | — | Anthropic provider, streaming, CLI, core skills |
| **Phase 2: Gateway** | 8 weeks | `abigail-gateway`, `abigail-channels` | WebSocket gateway, Telegram, Discord, Slack |
| **Phase 3: Execution** | 8 weeks | `abigail-sandbox` | Docker sandbox, vector memory, scheduler, workspaces |
| **Phase 4: Sensory** | 8 weeks | `abigail-browser` | Browser automation, voice, more messaging adapters |
| **Phase 5: Ecosystem** | 10 weeks | `abigail-registry` | Skill SDK, marketplace, mobile apps, canvas |
| **Total** | **~40 weeks** | **4 new crates** | **Feature parity with OpenClaw** |

### Effort by Category

| Category | Total Effort | Items |
|----------|-------------|-------|
| Skills ecosystem | XL + multiple M | Largest effort area — 100+ skills is a marathon |
| Messaging adapters | XL (cumulative) | Each adapter is M, but there are 10+ platforms |
| Browser automation | XL | CDP integration is complex, semantic snapshots add sophistication |
| Mobile apps | XL | Native app development is a separate discipline |
| Gateway architecture | L | Foundation for everything else |
| Memory enhancements | L (cumulative) | Vector search requires embedding infrastructure |
| Voice integration | L (cumulative) | Audio pipeline is non-trivial |
| LLM providers | M (cumulative) | Anthropic is highest priority |

---

## 8. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| **Scope creep** — 37 gaps is enormous for a small team | High | High | Strict phase gating; ship each phase before starting next |
| **Messaging API instability** — Platform APIs change frequently | Medium | High | Abstract adapter trait; version-pin SDKs; community maintenance |
| **Docker dependency** — Not all users will have Docker installed | Medium | Medium | Graceful fallback to in-process sandbox; WASM as alternative |
| **Mobile app maintenance** — iOS/Android are separate ecosystems | Medium | High | Consider React Native for shared codebase; or defer to Phase 5+ |
| **Skills quality vs quantity** — 100+ skills need maintenance | High | Medium | Community-driven with quality gates; curated "official" subset |
| **Voice latency** — Real-time audio requires low-latency pipeline | Medium | Medium | Start with push-to-talk; always-on as enhancement |
| **Rust async complexity** — Gateway + channels + sandbox = complex async | Medium | Medium | Leverage tokio ecosystem; comprehensive integration tests |
| **Losing Abigail's identity** — Chasing parity may dilute Abigail's strengths | Medium | High | Preserve constitutional identity as core differentiator; don't remove existing features |

---

## 9. Recommendations

### Strategic Recommendations

1. **Don't try to be OpenClaw.** Abigail has a unique philosophy — constitutional identity, psychoanalytic routing, cryptographic sovereignty. Use feature parity as a *capability* target, not an identity target.

2. **Prioritize the gateway.** The WebSocket gateway (Phase 2) unlocks everything: messaging, CLI, web interface, multi-agent, and mobile. It's the single highest-leverage architectural investment.

3. **Anthropic provider first.** OpenClaw recommends Claude as the primary model. Adding an `AnthropicProvider` immediately expands Abigail's LLM reach and aligns with the best available models.

4. **Skills are a community problem.** Building 100+ skills in-house is not feasible. Invest in the skill SDK and registry infrastructure (Phase 5.1-5.2), then grow through community contributions.

5. **Browser automation is a differentiator.** The semantic snapshot approach is novel and efficient. Implementing it in Rust (with CDP via chromiumoxide) could yield a faster, more memory-efficient implementation than OpenClaw's Node.js version.

6. **Defer mobile to late phases.** Mobile companion apps are high-effort, high-maintenance. The gateway + messaging adapters provide mobile *reach* without native apps.

### Tactical Recommendations

7. **Start Phase 1 immediately.** Adding Anthropic, streaming, and core skills has zero architectural risk and immediate user value.

8. **Use the existing skill infrastructure.** Abigail's `SkillRegistry`, `SkillExecutor`, `EventBus`, and TOML manifest system is solid. Build on it rather than replacing it.

9. **SQLite FTS5 before vector search.** Full-text search is simpler, faster to implement, and covers 80% of memory search needs. Add vector embeddings in Phase 3.

10. **Test each messaging adapter independently.** Each platform has different rate limits, authentication, and message formats. Ship one at a time.

---

## Appendix A: New Crate Proposals

### `abigail-gateway`
WebSocket server, session management, lane queue system, channel routing. Depends on: `abigail-router`, `abigail-skills`, `abigail-memory`.

### `abigail-channels`
Channel adapter trait + platform-specific implementations (Telegram, Discord, Slack, etc.). Depends on: `abigail-gateway`.

### `abigail-sandbox`
Docker container management via bollard crate. Execution context isolation for skill/tool execution. Depends on: `abigail-skills`.

### `abigail-browser`
Chrome DevTools Protocol client, page navigation, semantic snapshots, accessibility tree parsing. Depends on: `abigail-capabilities`.

### `abigail-registry` (Phase 5)
Remote skill catalog, version management, install/uninstall, dependency resolution. Depends on: `abigail-skills`.

---

## Appendix B: Quick Wins (Implementable This Week)

These items can be completed with minimal effort and provide immediate value:

1. **File system skill** — Read/write/list/search files (S effort)
2. **Shell command skill** — Execute shell commands with output capture (S effort)
3. **Skills watcher** — `notify` crate watches skill directories for changes (S effort)
4. **Anthropic API key validation** — Already exists in `validation.rs`, just wire it (S effort)
5. **Streaming stubs** — Add stream trait to `LlmProvider` even if not yet implemented (S effort)

---

*This document should be updated as implementation progresses. Each phase completion should trigger a review of priorities for subsequent phases.*
