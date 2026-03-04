# Changelog

All notable changes to Abigail are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- 2026-03-03 20:55 EST: Cross-platform vault overhaul — replace DPAPI-only encryption with AES-256-GCM envelope + HKDF-scoped key derivation, hybrid unlock (OS keyring / passphrase fallback), rewire SecretsVault/Keyring/encrypted_storage to new crypto core, add shared RESERVED_PROVIDER_KEYS, harden daemon secret namespace validation
- 2026-03-03 17:50 EST: Fix ChatInterface null storedProviders in tests — guard against null/undefined from get_stored_providers mock
- 2026-03-03 16:33 EST: Phase 4 Autonomous Job Delegation — fix provider/model dropdowns (whitelist chat models, merge vault providers), chat page job badge with live event stream, ego delegation tools (submit_background_job/get_job_result/list_my_jobs), capability-aware routing (ImageGeneration/AudioGeneration/VideoGeneration/Transcription), ExecutionMode Direct for skip-LLM skill dispatch, cron self-management tools, OrchestrationPanel event-driven updates, V6 schema migration
- 2026-03-03 17:30 EST: Complete Phase 3 Simplification, Observability, and UX — 3a: remove Council/TierBased routing (~2000 lines); 3b: Jobs tab with live JobQueue list and recurring templates; 3c: system prompt viewer in Identity/Soul tab; 3d: replace ForgePanel with Nerve Center topic monitor; 3e: Data Explorer with memory browse/search/sessions/stats tabs
- 2026-03-03 14:30 EST: Complete Phase 2c Memory and Integration — daemon-test-harness crate (HiveDaemonHandle, EntityDaemonHandle, TestCluster), hive-daemon and cross-daemon E2E tests, daemon-client crate (HiveDaemonClient, EntityClient with SSE), RuntimeMode config (InProcess/Daemon), DaemonManager for managed daemon lifecycle, chat_stream/list_skills/store_secret dual-mode delegation, DiagnosticsPanel runtime mode toggle
- 2026-03-03 13:15 EST: Complete Phase 2b Skill Creation — Dynamic API authoring docs, recursive discover (nested subdirs), SkillsWatcher for skill.toml+JSON hot-reload with register/unregister, StreamBroker skill events, Tauri watcher wiring with UI toast notifications
- 2026-03-04 16:30 EST: Add file upload and drag-and-drop to ChatInterface — paperclip file picker, drag overlay, attachment preview strip, save-to-entity-docs persistence, and get_entity_documents_path/save_to_entity_docs Tauri commands
- 2026-03-03 23:30 EST: Add BackupManagementSkill — entity self-restore: list/preview/import conversation turns and memories from backup SQLite databases into running MemoryStore
- 2026-03-04 14:00 EST: Fix 6 runtime bugs — author_skill secrets declaration, email SMTP config, filesystem allowed_roots expansion, OpenAI non-chat model filtering, perplexity reserved key, and provider-to-skill secret sync
- 2026-03-03 19:10 EST: Add OllamaDrawer (left) and ProviderDrawer (right) slide-out panels to SoulRegistry for pre-birth Ollama management and provider/CLI configuration
- 2026-03-03 17:55 EST: Add inline Provider + Model dropdowns to ChatInterface toolbar for instant model switching without leaving the conversation
- 2026-03-03 15:30 EST: Address 6 residual risks — wire server-side cancel for SSE streams (daemon + Tauri), expose governance constraints via routes and Tauri commands, remove legacy blocking chat command, deprecate target field across full stack, expose job scheduler via Tauri commands, add entity-cli skill-sign subcommand for signed allowlist entry generation
- 2026-03-03 13:40 EST: Fix Ollama startup lifecycle — resolve premature model_loading state, progress bar visibility for non-typewriter paths, double identity-check, pull button port mismatch, CI curl hardening, and fix download URLs to extract from archives (zip/tgz) instead of non-existent standalone binaries
- 2026-03-02 12:15 EST: Fix "Unsupported 16-Bit Application" error — validate bundled Ollama is a 64-bit PE before spawning, fall back to system Ollama when invalid
- 2026-03-02 23:20 EST: Fix startup flow — bundle Ollama in release-fast workflow, fix pull button parameter mismatch, show loading screen instead of skipping when Ollama unavailable
- 2026-03-02 21:40 EST: Fix release workflow version auto-increment failing on suffixed tags (e.g. v0.0.65-fast.4) — filter to clean semver tags only
- 2026-03-02 20:30 EST: Fix Ollama auto-connect skipping model download — add list_ollama_models backend command, guard auto-connect on installed models, replace button list with dropdown model picker (installed/recommended/custom)
- 2026-03-02 16:00 EST: Topic-based decomposition — kill EventBus for StreamBroker, async memory consumer, cron scheduling in JobQueue, lean orchestrator prompt, instruction topic-affinity, unified router method, async tool-use delegation with depends_on job chains, AgenticEngine StreamBroker events, conscience monitor topic consumer, council job fan-out
- 2026-03-01 01:00 EST: Fix model loading screen progress (serde event format mismatch) and auto-skip Ignition stage when managed Ollama is running with model ready
- 2026-02-28 23:30 EST: Bundle Ollama with ABNORMAL BRAIN loading screen — auto-start managed Ollama, pull Llama 3.2 3B on first launch with typewriter-flicker loading animation, graceful shutdown on exit, release workflow downloads and codesigns Ollama per-platform
- 2026-02-28 21:55 EST: Fix macOS notarization — codesign bundled abigail-keygen binary with hardened runtime and secure timestamp before Tauri build
- 2026-02-28 16:00 EST: Add sub-agent job queue Phase 1 — abigail-streaming crate (StreamBroker trait + MemoryBroker), abigail-queue crate (JobQueue with SQLite + dual-layer event publishing), V3 migration for job_queue table
- 2026-02-28 14:30 EST: Add Apple notarization to macOS release workflow and update CLAUDE.md with 5 missing crates, soul crystallization, agent backup/restore, and 12 frontend components
- 2026-02-28 12:30 EST: Fix birth cycle CLI indicators all turning green when one is enabled — only show active for explicitly activated providers
- 2026-02-28 04:55 EST: Add macOS Developer ID code signing to release workflow — import certificate into CI keychain, pass signing identity to Tauri build, clean up keychain after build
- 2026-02-27 14:30 EST: Rename skill-proton-mail to generic skill-email — remove Proton-specific defaults, make imap_host/smtp_host required, use Network=Full permissions
- 2026-02-28 02:00 EST: Agent backup/restore system, shared MemoryStore, chat turn persistence, confirmation modals, lenient tool param parsing, and Proton Mail skill auto-reinit on secret change
- 2026-02-28 01:00 EST: Ignition page auto-detects all providers on mount, highlights available tabs with green indicators, auto-focuses sole available option, remove redundant CLI Tools footer
- 2026-02-28 00:30 EST: Compress CLI system prompt (~15-41KB → ~1.5KB) with temp file spillover, budgeted instruction injection (max 1 / 2048 bytes for CLI), compact grouped tool list, and reduced max-turns to 5
- 2026-02-27 23:15 EST: Update docs for CliOrchestrator auto-detection, remove stale --allowedTools refs, mark completed Phase 2a/2c roadmap items, fix CliPermissionMode doc comments
- 2026-02-27 22:30 EST: Fix intermittent CLI spawn OS error 206 on Windows — replace broken --allowedTools args (entity tool names, not CLI tool names) with --dangerously-skip-permissions, auto-detect CliOrchestrator routing mode for CLI providers to skip meaningless tier scoring/complexity classification
- 2026-02-27 19:15 EST: CLI session management, memory auto-archive, and encrypted portable archives — pipe Claude CLI prompt via stdin to fix Windows 32K limit, add --resume/session_id for multi-turn, conversation_turns table and auto-archive in entity-daemon/Tauri, build_memory_context and prompt memory awareness, X25519+AES-256-GCM .abigail export to Documents/Abigail/archives/, restore via recovery key, auto-export every 50 turns
- 2026-02-27 21:00 EST: Birth/Entity chat pipeline separation — extract BirthChatEngine to abigail-birth crate with scripted no-LLM fallback, move key detection to abigail-core, add birth conversation persistence, slim Tauri commands to thin wrappers, add best_available_provider() to router
- 2026-02-27 15:30 EST: Full agentic CLI integration — CLI auth verification (official binary + auth status), CliOrchestrator routing mode, CLI Quick-Start in Ignition stage, rich command flags (--append-system-prompt, --output-format, streaming), CLI-optimized system prompt builder, routing mode selector in ChatInterface
- 2026-02-27 04:10 EST: Apply cargo fmt for CI lint gate
- 2026-02-27 03:50 EST: Enrich CLI REST server chat pipeline — system prompt, tool wiring, tool-use loop, full metadata; fix status to read live router state; register all 15 native Rust skills at Tauri startup
- 2026-02-26 22:20 EST: Add Insights troubleshooting panel — CLI REST server toggle with connection info, in-memory log ring buffer with reloadable tracing filter, live log viewer with stream/pause/clear/export, save-to-file via dialog
- 2026-02-26 20:15 EST: Routing simplification — make ExecutionTrace authoritative source of truth, add SelectionReason enum, implement explicit Council execution path, staged force overrides in Forge, diagnose_routing endpoint/command, operator runbook and migration docs
- 2026-02-26 16:30 EST: Entity-First Attribution — normalize chat-facing labels so Id never appears as conversational actor, map id(...) to "local" in UI and provider_label(), add entity-identity self-report rule to runtime prompt
- 2026-02-26 16:00 EST: Execution Truth Refactor — add execution_trace DTO and traced router paths, thread trace through entity-chat and Tauri/daemon responses, show configured+executed attribution and fallback chain in chat UI
- 2026-02-26 19:45 EST: Add entity runtime self-awareness — inject RuntimeContext (provider, model, tier, entity name) into system prompt, filter phantom skill instructions against registered skills, track provider change timestamps in config (schema v19)
- 2026-02-26 16:15 EST: Fix Tauri skill/secrets flow — register ProtonMailSkill at startup, unify secret namespace validation, bootstrap skill instructions into data_dir, add live E2E probe and PowerShell runner
- 2026-02-26 02:30 EST: Fix OpenAI/xAI/Google/local tool-name sanitization — extract shared sanitize_tool_name and build_tool_name_map to provider.rs, apply to all OpenAI-compatible providers (build_tools, build_messages, and reverse-map in complete/stream), consolidate Anthropic's local copy
- 2026-02-26 01:30 EST: Align streaming router path — refactor route_stream() to use target_for_mode() matching all other routing methods, remove dead EgoPrimary fallback code, extract shared stream_chat_pipeline() and provider_label() into entity-chat eliminating ~90 lines of duplication between Tauri and daemon
- 2026-02-25 22:15 EST: Add graceful local LLM error handling — parse JSON error bodies for friendly messages, add Id→Ego fallback on all router paths, and surface actionable guidance in chat UI for "no model loaded" and connection errors
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
