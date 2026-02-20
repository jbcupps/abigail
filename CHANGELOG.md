# Changelog

All notable changes to Abigail are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- 2026-02-19: Added 10 new skill crates (knowledge-base, git, code-analysis, database, calendar, document, notification, image, clipboard, system-monitor) + browser instruction-only skill, permission parser fix, productivity subagent, and expanded file_ops/privacy subagent capabilities
- 2026-02-19 20:45 EST: Implemented refined lightweight router – Fast Path (Id+Ego+Context) + out-of-band Superego/Trust conscience to eliminate lock-ups
- 2026-02-19: Added abigail-auth crate (Phase 1) — AuthProvider trait, StaticToken/BasicAuth providers, in-memory TokenCache, AuthManager registry with 11 tests
- 2026-02-19: feat(skills): add preloaded integration skills (GitHub, Slack, Jira) with auth framework wiring — embedded DynamicApiSkill configs, AuthManager in AppState, versioned bootstrap, get_integration_status/store_integration_credential commands, check_integration_status LLM tool, instruction registry entries
- 2026-02-19 23:30 EST: Fix credential storage refusal (conscience allowlist + enriched safety prompt + configure_email LLM tool) and add abigail-cli crate with CLI subcommands + REST troubleshooting API
- 2026-02-20 00:15 EST: Register ProtonMailSkill in runtime, add get_system_diagnostics LLM tool, troubleshooting instruction keywords, and fix skill-proton-mail compile errors
- 2026-02-19 14:30 EST: Unified UI theme system across all 20 components — chat bubbles, softer backgrounds, focus rings, scrollbar theming, ARIA accessibility, and zero hardcoded gray/blue colors
- Public release readiness: LICENSE, CONTRIBUTING.md, CODE_OF_CONDUCT.md, SECURITY.md
- CI workflow for pull request validation (cargo fmt, clippy, test, frontend build)
- CodeQL static analysis workflow
- GitHub issue and PR templates
- CODEOWNERS file
- All GitHub Actions pinned by commit SHA for supply chain security

### Changed

- Enhanced README.md with badges, system requirements, and troubleshooting
- Updated .gitignore with additional patterns for generated and data files

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
