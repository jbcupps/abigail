# Changelog

All notable changes to Abigail are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

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
