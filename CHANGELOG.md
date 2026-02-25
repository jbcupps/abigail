# Changelog

All notable changes to Abigail are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- 2026-02-25 17:40 EST: Implement streaming chat (Tauri events + daemon SSE), wire SkillFactory runtime auto-registration with DynamicApiSkill output, add E2E parity tests with mock providers across entity-chat/daemon/frontend
- 2026-02-24 23:00 EST: Implement SMTP client, wire ProtonMailSkill send_email, add live email E2E tests (entity-chat), fix Anthropic tool-name sanitization for all skill tool-use, extract HiveOperations trait for Tauri/daemon parity
- 2026-02-24 17:30 EST: Add tabula rasa UAT harness with STARTTLS IMAP support, entity-daemon --data-dir isolation, Hive-to-entity secret sync, and automated 9-stage test pipeline
- 2026-02-26 12:00 EST: Remove Superego from entity entirely; add ChatMemoryHook at entity memory persist as sole hook for future Hive/Superego use
- 2026-02-26 01:00 EST: Implement tier-based model routing overhaul — 3-tier model selection (Fast/Standard/Pro) with complexity scoring, force overrides (pin tier/model/provider+tier), dynamic model registry with provider API discovery and 24h TTL caching, per-request model_override in CompletionRequest, remove IdPrimary routing mode, add tier metadata to ChatResponse, and build full Forge UI with tier assignment grid, force override controls, and threshold tuning
- 2026-02-25 06:00 EST: Extract shared chat engine into entity-chat library crate, eliminating ~200 lines of duplicated pipeline code between entity-daemon and Tauri app so CLI and GUI use a single engine
- 2026-02-25 04:30 EST: Fix HiveManagementSkill tool schemas causing OpenAI 400 errors, add defensive schema validation in build_tool_definitions, implement dynamic model discovery (discover_models API + Hive endpoint + startup diagnostics)
- 2026-02-25 03:00 EST: Strip chat flow to standard minimum — remove redundant tool awareness markdown, dead "Blocked:" postfix, and write-only memory persistence from both GUI and CLI pipelines (10→6 steps GUI, 8→5 steps CLI)
- 2026-02-25 02:00 EST: Unify GUI and CLI chat flows — replace streaming chat_stream with non-streaming chat command mirroring entity-daemon pipeline (tool-use loop, tool awareness, memory persistence), delete ~650 lines of superfluous code, simplify ChatInterface.tsx frontend
- 2026-02-24 15:30 EST: Wire abigail-memory into entity-daemon with REST endpoints, chat persistence, memory CLI commands, tool-use loop, skill auto-loading, and skill scaffolding CLI
- 2026-02-23 22:00 EST: Add full chat pipeline to entity-daemon (system prompt, message sanitization, tool awareness, deduplication, risk clarification) for parity with Tauri app
- 2026-02-23 18:30 EST: Disable Tauri updater artifact signing (createUpdaterArtifacts: false) to fix release build failure from malformed signing key
- 2026-02-23 18:00 EST: Update all documentation to reflect Hive/Entity daemon architecture; remove superfluous MVP_SCOPE, PHASE1_AGILE_PLAN, and prompt-routing-review docs
- 2026-02-23 16:00 EST: Implement Hive/Entity separation Phase 1 — seven new crates (hive-core, entity-core, abigail-identity, hive-daemon, entity-daemon, hive-cli, entity-cli) splitting the Tauri monolith into separate control-plane and agent-runtime HTTP daemons
- 2026-02-22 19:00 EST: Create abigail-hive crate as single authority for secret resolution and provider construction, moving builder functions out of router and Tauri app
- 2026-02-22 01:30 EST: Harden release signing key pipeline to auto-strip whitespace from base64, validate encoding, and pass sanitized key to build step
- 2026-02-22 00:10 EST: Fix CI lint gate by correcting import ordering in config.rs
- 2026-02-21 23:20 EST: Fix native birth/chat provider reliability and improve Forge feedback/readability while hardening browser harness tests to catch real-world provider failure and recovery behavior.
- 2026-02-21 19:30 EST: Fix TAURI signing-key validator escaped-newline detection so CI preflight tests pass for valid minisign secret formats.
- 2026-02-21 19:15 EST: Add lock-safe MCP server URL resolution and focused clippy guard to catch lock-across-await regressions in abigail-app CI.
- 2026-02-21 15:40 EST: Add release signing-key preflight validation and CI regression test to catch malformed TAURI updater secret format before Tauri bundling.
- 2026-02-21 15:00 EST: LLM routing and skills verification — unified router rebuild/superego, trust gating and capability envelope in execution, qualified tool resolution, routing tests, and vision chunk alignment (birth, identity, chat, forge, skills).
- 2026-02-21 12:00 EST: Refresh documentation baseline by aligning README, CLAUDE guidance, release process, GitHub settings checklist, and environment notes with current CI/release workflows.
- 2026-02-20 17:30 EST: Massive cleanup and rebranding to Sovereign Entity model: modularized Tauri backend, pruned legacy routing/stubs, rebranded UI to Soul Registry and Sanctum, and implemented agentic Recall memory tool.
- 2026-02-19 19:14 EST: Added 10 new skill crates (knowledge-base, git, code-analysis, database, calendar, document, notification, image, clipboard, system-monitor) + browser instruction-only skill, permission parser fix, productivity subagent, and expanded file_ops/privacy subagent capabilities
- 2026-02-19 20:45 EST: Implemented refined lightweight router – Fast Path (Id+Ego+Context) + out-of-band Superego/Trust conscience to eliminate lock-ups
- 2026-02-19 13:34 EST: Added abigail-auth crate (Phase 1) — AuthProvider trait, StaticToken/BasicAuth providers, in-memory TokenCache, AuthManager registry with 11 tests
- 2026-02-19 19:02 EST: Add preloaded integration skills (GitHub, Slack, Jira) with auth framework wiring — embedded DynamicApiSkill configs, AuthManager in AppState, versioned bootstrap, get_integration_status/store_integration_credential commands, check_integration_status LLM tool, instruction registry entries
- 2026-02-19 23:30 EST: Fix credential storage refusal (conscience allowlist + enriched safety prompt + configure_email LLM tool) and add abigail-cli crate with CLI subcommands + REST troubleshooting API
- 2026-02-20 00:15 EST: Register ProtonMailSkill in runtime, add get_system_diagnostics LLM tool, troubleshooting instruction keywords, and fix skill-proton-mail compile errors
- 2026-02-19 14:30 EST: Unified UI theme system across all 20 components — chat bubbles, softer backgrounds, focus rings, scrollbar theming, ARIA accessibility, and zero hardcoded gray/blue colors
- 2026-02-20 10:30 EST: UI overhaul — replace single-line input with auto-growing textarea, remove top tab bar, add slide-out Forge drawer with 10 sub-tabs, expose get_system_diagnostics as Tauri command, add DiagnosticsPanel
- 2026-02-20 14:30 EST: Add Tauri auto-update plugin with UpdateNotification banner, fix NSIS backup for Hive multi-agent files, add SQLite migration framework with schema_versions table, and UPGRADE.md documentation
- 2026-02-20 15:30 EST: Add CLI provider adapter for Claude Code, Gemini CLI, OpenAI Codex CLI, and xAI Grok CLI as Ego routing backends
- 2026-02-20 16:00 EST: Add CLI provider options to birth Ignition/Connectivity stages, post-birth config menu, and LLM tool schema
- 2026-02-20 19:30 EST: Fix CI gate by committing missing auto-updater deps, schema migration code, capabilities, and UpdateNotification component
- 2026-02-19 10:55 EST: Public release readiness: LICENSE, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, CI workflows, CodeQL, and CODEOWNERS

### Changed

- 2026-02-18 23:04 EST: Enhanced README.md with badges, system requirements, and troubleshooting
- 2026-02-18 01:36 EST: Updated .gitignore with additional patterns for generated and data files

## [0.0.1] - 2026-02-03

### Added

- Initial release of Abigail desktop agent
- Interactive birth flow with staged onboarding
- First-run Ed25519 signing key generation with one-time private key presentation
- Constitutional document signing and verification (soul.md, ethics.md, instincts.md)
- Local LLM discovery and manual connect for Ollama/LM Studio-compatible endpoints
- In-app API key vaulting and validation for cloud/model/search providers
- Dual persona UI modes (surface chat and Forge mode toggle)
- Id/Ego routing: local LLM (Id) for routine queries, cloud LLM (Ego) for complex queries
- Skill-based tool execution with web-search capability
- DPAPI-encrypted secrets storage on Windows
- Cross-platform builds: Windows (NSIS), Ubuntu (deb), macOS (dmg universal binary)
- npm CLI installer (`npx abigail-desktop`)
- Docker development and build containers
- Security audit CI (cargo audit, npm audit)
- Dependabot configuration for Cargo, npm, and GitHub Actions

[Unreleased]: https://github.com/jbcupps/abigail/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/jbcupps/abigail/releases/tag/v0.0.1
