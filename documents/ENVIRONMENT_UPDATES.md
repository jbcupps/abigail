# Environment Updates

Dated log of environment, dependency, CI, container, or infrastructure changes. No sensitive data.

## 2026-02-23 (Hive/Entity Separation Phase 1)

- **Architecture:** Implemented Hive/Entity separation — seven new crates (`hive-core`, `entity-core`, `abigail-identity`, `hive-daemon`, `entity-daemon`, `hive-cli`, `entity-cli`) splitting the monolith into independent control-plane and agent-runtime HTTP daemons.
- **Workspace:** Added 7 new members and 6 workspace-level dependencies (`axum`, `clap`, `tower-http`, `hive-core`, `entity-core`, `abigail-identity`).
- **Identity extraction:** `tauri-app/src/identity_manager.rs` (645 lines) extracted to `abigail-identity` crate; tauri-app re-exports via `pub use abigail_identity::*`.
- **Hive daemon:** Axum server on `:3141` wrapping `IdentityManager` + `Hive` + `SecretsVault`. 9 endpoints for identity, secrets, and provider config resolution.
- **Entity daemon:** Axum server on `:3142` wrapping `IdEgoRouter` + `SkillRegistry` + `SkillExecutor`. Fetches provider config from Hive on startup. `HttpHiveOps` replaces `TauriHiveOps` for the `HiveOperations` trait.
- **Documentation:** Updated CLAUDE.md, README.md, CONTRIBUTING.md, HOW_TO_RUN_LOCALLY.md, RELEASE.md, SECURITY_NOTES.md, THREAT_MODEL.md, Feature_Gap_Analysis.md, USER_EXPERIENCE.md, UPGRADE.md, GITHUB_SETTINGS.md. Removed superfluous docs: MVP_SCOPE.md, PHASE1_AGILE_PLAN.md, prompt-routing-review.md.

## 2026-02-21 (Workspace refresh baseline after sovereign refactor wave)

- **Branch + drift snapshot:** Active branch is `feat/autonomous-skills-factory` with local, uncommitted edits across router, config, capability providers, Tauri command modules, and UI surfaces (`ChatInterface`, `ForgePanel`). Untracked local artifacts include `.cargo/`, `.grok/`, `tauri-app/src-ui/coverage/`, root `node_modules/`, and `nul`.
- **Workspace shape:** `Cargo.toml` workspace includes expanded architecture: core crates (`abigail-core`, `abigail-memory`, `abigail-router`, `abigail-capabilities`, `abigail-birth`, `abigail-skills`), newer support crates (`abigail-auth`, `abigail-cli`, `abigail-soul-crystallization`, `soul-forge`), and a broad skill pack (filesystem/shell/http/search plus git, database, code-analysis, image, notification, calendar, clipboard, system-monitor, etc.).
- **Operational model (current):** Product positioning and docs now center on Sovereign Entity operations (Hive > Entity > Agent), Sanctum/Superego framing, multi-identity management, and autonomous self-configuration/skill-factory pathways.
- **Recent delivery cadence:** Current branch history confirms rapid iterative shipping (router/provider expansion, CLI provider support, UI forge/drawer overhauls, diagnostics, updater + migration work, CI stabilization), so new work should assume active in-flight refactors rather than a clean release baseline.

## 2026-02-08 (Tests: executor timeout/sandbox, MCP schema mapping)

- **abigail-skills**: `SkillExecutor::with_limits(registry, limits)` added for tests. Executor now uses stored `default_timeout_ms` for per-call timeout. Tests added: `executor_enforces_timeout` (short limit + sleeping skill → timeout error), `executor_denies_network_when_not_granted` (tool requires network, manifest has no permission → PermissionDenied). MCP: `mcp_tool_to_descriptor_maps_name_and_schema` in `protocol/mcp.rs` for schema mapping.

## 2026-02-08 (Docs, threat model, Docker-first dev)

- **Security docs**: `documents/SECURITY_NOTES.md` updated with skill packaging and approval, audit log, signing path, MCP trust (server definitions, trust policy, tool confirmation), and resource limits (timeouts and concurrency enforced in executor). Threat table extended for skill supply-chain, MCP exfiltration, and UI sandbox escape.
- **Threat model**: Added `documents/THREAT_MODEL.md` covering skill supply-chain abuse, MCP server trust, UI sandbox escape (MCP Apps), and data exfiltration, with mitigations and abuse cases.
- **Docker**: Added `docker/Dockerfile` (Rust + Node + Tauri system deps for Debian), `docker/docker-compose.yml` (services `abigail-dev` and `abigail-build` with repo bind-mount and cargo cache volume), and `docker/.dockerignore`. `documents/HOW_TO_RUN_LOCALLY.md` updated to reference `docker/` and a build step before starting the dev container.

## 2026-02-08 (Align CI and release with jbcupps/AO model)

- **CI (`.github/workflows/ci.yml`)**: Aligned with AO-style quality gate. Triggers on push to main, pull_request to main, and weekly schedule (CodeQL). Runners set to `ubuntu-22.04` where applicable. All actions pinned by commit SHA (checkout, rust-toolchain, rust-cache, setup-node, codeql-action). CodeQL job added (JavaScript/TypeScript, advisory, weekly). Gate job now depends on lint, test, frontend, audit, and codeql; lint/test/frontend are required, audit and codeql advisory.
- **Release (`.github/workflows/release.yml`)**: Refactored to two-stage AO model. Stage 1: single `build` job with matrix (windows-latest, ubuntu-22.04, macos-latest). Version determined in build job (tag, manual input, or auto-increment). Tauri config patched (version, disable beforeBuildCommand for CI, platform resources). Build outputs upload as artifacts (`abigail-installer-<platform>`). Stage 2: `publish` job runs when build finishes (success or failure); downloads artifacts, renames to stable asset names, creates GitHub Release (non-draft) with `release-assets/*`, then publishes npm package. Workflow dispatch input renamed to `release_version` (optional). Actions pinned by SHA.
- **Build (`.github/workflows/build.yml`)**: Retained unchanged. Used for push-to-main verification only; does not create releases. Documented in `documents/RELEASE.md`.
- **Docs**: `documents/RELEASE.md` updated to describe Release workflow (two-stage), manual dispatch, and build.yml role. `documents/GITHUB_SETTINGS.md` already referenced gate and codeql; no change.

## 2026-02-07 (Fix release build failure: missing anyhow)

- **CI/Release**: Added `anyhow` dependency to `tauri-app/Cargo.toml` so `identity_manager.rs` can compile during `tauri build`.
- **Context**: Release workflow failed on Windows/macOS/Linux in "Build Tauri app" with `E0433` (unresolved crate `anyhow`).

## 2026-02-07 (Fix PR test failures on claude/refactor-hive-architecture-PglPx)

- **Tests**: Replaced Unix-only paths in unit tests so CI passes on Windows (and all platforms).
  - **abigail-core** `global_config.rs`: `test_find_and_remove_agent` no longer uses `PathBuf::from("/tmp")`; uses `std::env::temp_dir().join("ao_global_config_find_remove")` with create/cleanup.
  - **abigail-skills** `watcher.rs`: `test_watcher_handles_nonexistent_dir` no longer uses `PathBuf::from("/tmp/ao_watcher_nonexistent_12345")`; uses `std::env::temp_dir().join("ao_watcher_nonexistent_12345")`.
- **Note**: CI runs `cargo test --workspace --exclude abigail-app`; the Tauri app build (e.g. Docker full build) still expects resource `abigail-keygen.exe` and may fail on Linux until bundle config is made cross-platform (e.g. externalBin).

## 2026-02-07 (Tabula rosa UAT skill and findings folder)

- **New**: `.cursor/skills/tabula-rosa-uat/SKILL.md` — Cursor skill for running tabula-rosa UAT: clean environment, Windows installer + Docker + npm testing, findings backup, cleanup.
- **.gitignore**: Added `uat-findings/` so UAT run reports can be written to a gitignored folder at repo root.

## 2026-02-06 (Fix CI workflow failures after consolidation)

- **lint**: Removed "Install Linux dependencies" step; lint only runs fmt/clippy and does not need WebKit/GTK.
- **codeql**: Set `continue-on-error: true` so the workflow does not fail when Code scanning is not enabled for the repo.
- **test**: Run `cargo test --workspace --exclude abigail-app` so the Tauri app (which requires the bundled abigail-keygen binary) is not built during tests; all library crates are still tested.
- **Ubuntu deps**: Aligned with Tauri 2 official Debian prerequisites (build-essential, curl, wget, file, libxdo-dev; dropped libappindicator3-dev). Applied in both ci.yml (test job) and release.yml.
- **release publish job**: Condition set to run when build completes with success or failure (so partial build artifacts can still be released).

## 2026-02-06 (Consolidate GitHub Actions: 5 workflows to 2)

Reduced CI/CD clutter by consolidating 5 workflow files into 2.

### Deleted

- `.github/workflows/security-audit.yml` -- merged into `ci.yml` as `audit` job
- `.github/workflows/codeql.yml` -- merged into `ci.yml` as `codeql` job
- `.github/workflows/build-release.yml` -- replaced by `release.yml`
- `.github/workflows/npm-publish.yml` -- merged into `release.yml` as final stage

### Created / Rewritten

- **`.github/workflows/ci.yml`** (rewritten): Unified quality gate with 5 parallel jobs (`lint`, `test` 3-platform matrix, `frontend`, `audit`, `codeql`) feeding into a `gate` aggregator job. Gate fails only if ALL upstream jobs failed. Single required status check for branch protection. CodeQL also runs on weekly schedule.
- **`.github/workflows/release.yml`** (new): Combined build + release + npm-publish pipeline. Triggers on `v*` tags and manual dispatch only (no longer auto-releases on every push to main). Stage 1: parallel 3-platform builds. Stage 2: create GitHub Release and publish npm package, proceeding if at least one platform built successfully.

### Updated

- `documents/GITHUB_SETTINGS.md` -- branch protection now references single `gate` check instead of per-workflow checks.

## 2026-02-05 (Public release readiness)

Comprehensive preparation for making the repository public. No secrets, credentials, or sensitive data were found in the audit.

### New Files Created

- **`LICENSE`**: MIT license file (was declared in Cargo.toml but file was missing).
- **`CONTRIBUTING.md`**: Contribution guidelines covering dev setup, branching strategy, Conventional Commits, PR process, and code style.
- **`CODE_OF_CONDUCT.md`**: References Contributor Covenant v2.1 with project-specific reporting instructions.
- **`CHANGELOG.md`**: Version history in Keep a Changelog format, documenting v0.0.1 and unreleased changes.
- **`.github/SECURITY.md`**: Vulnerability reporting policy with responsible disclosure instructions, scope, and security practices summary.
- **`.github/PULL_REQUEST_TEMPLATE.md`**: PR template with type checkboxes, testing checklist, and review checklist.
- **`.github/ISSUE_TEMPLATE/bug_report.yml`**: Structured bug report form (version, platform, steps to reproduce).
- **`.github/ISSUE_TEMPLATE/feature_request.yml`**: Feature request form (problem, solution, area).
- **`.github/ISSUE_TEMPLATE/config.yml`**: Issue template config with security and docs contact links.
- **`.github/CODEOWNERS`**: Default code owner (@jbcupps) for all paths.
- **`.github/workflows/ci.yml`**: New CI workflow for PR validation -- runs `cargo fmt --check`, `cargo clippy`, `cargo test --all`, and frontend build on Windows, Ubuntu, and macOS matrix.
- **`.github/workflows/codeql.yml`**: CodeQL static analysis for JavaScript/TypeScript on PRs, pushes to main, and weekly schedule.
- **`documents/GITHUB_SETTINGS.md`**: Checklist of manual GitHub repository settings (branch protection, security features, actions config, secrets).

### Modified Files

- **`.github/workflows/build-release.yml`**: All GitHub Actions pinned by commit SHA (actions/checkout, actions/setup-node, dtolnay/rust-toolchain, swatinem/rust-cache, actions/upload-artifact, actions/download-artifact, softprops/action-gh-release). Version tag preserved in comment for readability.
- **`.github/workflows/npm-publish.yml`**: Pinned actions/checkout and actions/setup-node by commit SHA.
- **`.github/workflows/security-audit.yml`**: Pinned actions/checkout and actions/setup-node by commit SHA.
- **`.gitignore`**: Added patterns for `secrets.bin`, `keys.bin`, `*.pdb`, `tauri-app/gen/`, `config.json`, `abigail_seed.db` (and WAL/SHM), `external_pubkey.bin`.
- **`tauri-app/tauri.conf.json`**: Set `copyright` field to "Copyright (c) 2025-2026 Jim Cupps".
- **`README.md`**: Added CI/security/license badges, system requirements table, end-user vs developer quick start, environment variables table, troubleshooting section, links to all new documentation.

### Security Audit Findings

- No real secrets, API keys, passwords, or sensitive data found in the repository.
- Test credentials in unit tests are clearly placeholder values (e.g., `sk-test-key-123`).
- All API endpoints are public (OpenAI, Anthropic, Tavily, Google).
- `.env` files properly gitignored; `example.env` contains only empty placeholders.
- GitHub Actions use `secrets.GITHUB_TOKEN` and `secrets.NPM_TOKEN` correctly via GitHub Secrets.

## 2026-02-05 (Multi-platform delivery: macOS, Ubuntu, Docker, npm)

Expanded Abigail distribution from Windows-only to four delivery channels.

### CI/CD: Cross-platform builds (`.github/workflows/build-release.yml`)

- **Build matrix expanded** from Windows-only to three platforms:
  - `windows-latest` (x86_64, NSIS `.exe`)
  - `ubuntu-22.04` (x86_64, `.deb`)
  - `macos-latest` (universal binary via `lipo`, `.dmg`)
- **Platform-specific CI steps**: Ubuntu installs `libwebkit2gtk-4.1-dev` and Tauri Linux deps; macOS adds `aarch64-apple-darwin` + `x86_64-apple-darwin` targets; Windows installs NSIS (conditionally).
- **abigail-keygen**: Built as universal binary on macOS (`lipo`); binary name set per-platform via matrix variable (`abigail-keygen.exe` on Windows, `abigail-keygen` elsewhere).
- **`tauri.conf.json` resources**: Patched dynamically in CI per platform to use correct binary name.
- **Icons step**: Changed from PowerShell to cross-platform bash.
- **Release job**: Collects artifacts from all three platforms; renames to stable asset names (`Abigail-windows-x64-setup.exe`, `Abigail-linux-x64.deb`, `Abigail-macos-universal.dmg`). Release notes updated with all platform download links and platform-specific notes (Gatekeeper, Linux deps).

### Tauri config (`tauri-app/tauri.conf.json`)

- Added `bundle.linux.deb` config: `depends` lists `libwebkit2gtk-4.1-0` and `libayatana-appindicator3-1`; `section: "utils"`.
- Added `bundle.macOS.minimumSystemVersion: "10.15"`.

### Docker (`docker/`)

- **`docker/Dockerfile.dev`**: Development container based on `rust:1.84-bookworm` with Node.js 20, Tauri Linux system deps, `cargo-audit`, `tauri-cli`. Non-root user. Entrypoint: bash.
- **`docker/Dockerfile`**: Multi-stage build for validation. Builder stage compiles workspace + runs tests. Runtime stage uses `debian:bookworm-slim` with minimal deps, non-root user.
- **`docker/docker-compose.yml`**: `abigail-dev` service (bind-mount dev shell, port 1420) and `abigail-build` service (one-shot validation).
- **`docker/.dockerignore`**: Excludes `target/`, `node_modules/`, `.git/`, secrets, IDE files.

### npm package (`npm-package/`)

- **`abigail-desktop`** npm package: CLI wrapper (`npx abigail-desktop`) that detects OS/arch, downloads the correct installer from GitHub Releases latest, and runs platform-specific install logic (NSIS on Windows, DMG mount+copy on macOS, dpkg on Linux).
- Zero runtime dependencies; uses only Node.js built-ins (`node:https`, `node:fs`, `node:child_process`, `node:os`).
- Commands: `install` (default), `version`, `help`.
- **`.github/workflows/npm-publish.yml`**: Publishes `abigail-desktop` to npm when a GitHub Release is published. Requires `NPM_TOKEN` repo secret.

### Documentation

- **`documents/RELEASE.md`**: Updated with all four delivery channels (direct download, npm CLI, Docker), platform-specific notes table, and npm publishing instructions.
- **`documents/HOW_TO_RUN_LOCALLY.md`**: Added Docker quick-start section, platform-specific prerequisites, npm install instructions.

## 2026-02-05 (Security audit CI and Dependabot)

- **`.github/workflows/security-audit.yml`:** New workflow runs on push to main and on pull requests. Runs `cargo audit` and `npm audit --audit-level=high` (tauri-app/src-ui). Fails on high/critical advisories.
- **`.github/dependabot.yml`:** Added for Cargo, npm (tauri-app/src-ui), and GitHub Actions with weekly schedule and PR labels. See SECURITY_NOTES.md for dependency and CI security.

## 2026-02-03 (Fix rust-cache "invalid toolchain name ''" on Windows)

- **Cause:** `swatinem/rust-cache` runs `rustc + $toolchain` to build the cache key. When `dtolnay/rust-toolchain` is used without an explicit `toolchain:` input (relying only on `rust-toolchain.toml`), it can leave the toolchain name empty on some runners, so rust-cache fails with `error: invalid toolchain name ''`.
- **Fix:** Added explicit `toolchain: stable` to the Setup Rust step in both `.github/workflows/build-release.yml` and `.github/workflows/build-release-deva.yml`. Matches `rust-toolchain.toml` (channel = "stable") and ensures rust-cache always has a non-empty toolchain.
- **Deva:** Also reverted "Update version in tauri.conf.json" to use the same bash/jq/sed approach as master for consistency.

## 2026-02-03 (Deva build troubleshooting and D 0.0.0)

- **Deva workflow (`build-release-deva.yml`):**
  - Default Deva version set to **D 0.0.0** (version `0.0.0`) when no tag or input.
  - Checkout uses `ref: ${{ github.ref }}` so both tag (`refs/tags/deva-v*`) and branch (`refs/heads/Deva`) work.
  - Trigger on **push to branch Deva** added so every push to Deva runs the build (artifacts only; no release). Use this to monitor the build.
  - Version update step uses Node (instead of jq) so it runs on Windows runners where jq is not installed.
  - `workflow_dispatch` input default set to `0.0.0`.

- **Past CI failures (from logs):** Build had failed on Windows due to `abigail-skills`: (1) `as_table()` returns `Option<&Map>`, not `Option<Value>` — repo already uses `if let Some(t) = s.permission.as_table()`. (2) `SkillId` must implement `Display` for thiserror — repo already has `impl Display for SkillId`. No code change needed if current Deva branch has those fixes.

To release Deva 0.0.0:
```bash
git checkout Deva
git tag deva-v0.0.0
git push origin deva-v0.0.0
```
Or: Actions → build-release-deva → Run workflow (branch: Deva, release version: 0.0.0).

## 2026-02-03 (Deva release workflow)

Added separate GitHub Actions workflow for Deva branch releases:

- **`.github/workflows/build-release-deva.yml`:** Mirrors `build-release.yml` but configured for Deva branch.
  - Triggers on `deva-v*` tags (e.g., `deva-v0.0.0`) or manual `workflow_dispatch`
  - Creates **pre-release** (not marked as latest) so stable releases remain prominent
  - Artifacts named `Abigail-Deva-*` to distinguish from stable releases
  - Release notes explain this is a development/preview build

## 2026-02-03 (First-run keypair generation + installer alignment)

Major change to external signing key flow - keypair now generated at first run instead of out-of-band:

### Backend Changes

- **`abigail-core/src/keyring.rs`:** Added `generate_external_keypair()`, `sign_document()`, `sign_constitutional_documents()`, `parse_private_key()` functions. External keypair is generated at first run; private key returned to UI for user to save; only public key stored.
- **`abigail-core/src/document.rs`:** Updated `CoreDocument::signable_bytes()` to use format `{name}|{tier:?}|{content}` for consistency with signing tools.
- **`abigail-core/src/config.rs`:** Added `effective_external_pubkey_path()` method that auto-detects `{data_dir}/external_pubkey.bin` if no explicit path configured.
- **`abigail-birth/src/stages.rs`:** Updated `verify_crypto()` to use `effective_external_pubkey_path()`.
- **`tauri-app/src/lib.rs`:** New Tauri commands `generate_and_sign_constitutional` and `has_external_keypair`. Modified `init_soul` to no longer copy signature files (signatures generated at first run). Updated `run_startup_checks` to use `effective_external_pubkey_path()`.
- **`tauri-app/src/templates.rs`:** Removed placeholder signature constants; signatures now generated dynamically.

### Frontend Changes

- **`BootSequence.tsx`:** Added `KeyPresentation` stage with:
  - Display of base64-encoded private key (Ed25519)
  - Copy-to-clipboard functionality
  - Security warnings (red box) explaining key importance
  - Checkbox acknowledgment required before proceeding
  - Private key cleared from state after acknowledgment

### CI Changes

- **`.github/workflows/build-release.yml`:**
  - Pinned all actions by SHA for reproducibility
  - Added version sync from git tag to `tauri.conf.json`
  - Added structured release notes explaining first-run key generation
  - Release body includes installation instructions and security note

### Documentation

- **`documents/SECURITY_NOTES.md`:** Complete rewrite documenting new key management model, first-run security flow, and threat model summary.

### Security Model Change

| Aspect | Before | After |
|--------|--------|-------|
| Who generates signing key | Developer (out-of-band) | End user (at first run) |
| When templates are signed | Build time | First run |
| Private key location | Developer's secure storage | User's secure storage |
| Trust model | "Developer signed these docs" | "User owns their instance's integrity" |

## 2026-02-03 (Startup order, external vault, LiteLLM heartbeat)

Major refactor of startup flow and signature verification:

- **External vault:** Added `abigail-core/src/vault.rs` with `ExternalVault` trait and `ReadOnlyFileVault` implementation. The signing public key is now read from an external file (outside Abigail's data dir) that Abigail can read but not write. Private signing key is created out-of-band (GPG, OpenSSL, or `scripts/generate-signing-key.ps1`).
- **Keyring v2:** Updated `Keyring` to no longer generate or store the install signing key. Only the mentor keypair is stored internally. Legacy v1 format (with install_pubkey) is still readable for migration.
- **Verifier:** Updated to use external public key from vault instead of internal keyring. `Verifier::from_vault(&vault)` creates a verifier with the external trust root.
- **LiteLLM HTTP provider:** Added `abigail-llm/src/local_http.rs` with `LocalHttpProvider` for OpenAI-compatible local LLM servers (LiteLLM, Ollama, LM Studio). Includes `heartbeat()` method for startup check.
- **Router:** Updated `IdEgoRouter::new(local_llm_base_url, openai_api_key)` to use HTTP provider when URL is set, otherwise falls back to Candle stub. Added `heartbeat()` and `is_using_http_provider()` methods.
- **Config:** Added `external_pubkey_path: Option<PathBuf>` and `local_llm_base_url: Option<String>` to `AppConfig`.
- **Startup checks:** New Tauri command `run_startup_checks` runs LLM heartbeat then signature verification. Returns `{ heartbeat_ok, verification_ok, error }` for UI to show status.
- **Birth shortcut:** Added `skip_to_life_for_mvp()` to birth orchestrator for streamlined first-run (skips email and model download).
- **init_soul:** Now copies pre-signed templates + .sig files instead of signing at runtime.
- **Frontend:** Simplified `BootSequence.tsx` to single Start flow: init_soul → run_startup_checks → show "Abigail informed OK" → complete birth → chat. `App.tsx` runs startup checks on every launch when already born.
- **Scripts:** Added `scripts/generate-signing-key.ps1` to generate Ed25519 keypair and sign templates out-of-band.
- **Docs:** Updated `example.env`, `HOW_TO_RUN_LOCALLY.md`, `MVP_SCOPE.md` with new startup order and external signing key instructions.

Dev mode: If `external_pubkey_path` is not set, signature verification is skipped with a warning. If `local_llm_base_url` is not set, heartbeat uses in-process stub (always succeeds).

## 2026-02-03 (README)

- **Docs:** Added root `README.md` — project intro, quick start (Rust + Node, `cargo tauri dev`), project layout table, links to documents (HOW_TO_RUN_LOCALLY, MVP_SCOPE, RELEASE, SECURITY_NOTES), license (MIT).

## 2026-02-03 (Auto-publish releases)

- **CI (build-release):** Changed `draft: true` to `draft: false` so releases are published immediately after the workflow completes. Users can now see the release and download installers from the Releases section without manual publishing.

## 2026-02-03 (Download page for installers)

- **CI (build-release):** In the release job, added step "Rename to stable asset names for latest/download URLs" after downloading artifacts. Copies the single .exe, .deb, and .dmg from each artifact dir into `release-assets/` as `Abigail-windows-x64-setup.exe`, `Abigail-linux-x64.deb`, `Abigail-macos-x64.dmg`. Release now attaches these fixed names so `https://github.com/jbcupps/abigail/releases/latest/download/<filename>` always points at the latest published release.
- **Download page:** Added `docs/index.html` — single static page with OS detection (userAgent/platform), one primary "Download for Windows/macOS/Linux" button linking to the stable latest-release URL, and "Other downloads" listing all three platforms. No build step; for GitHub Pages from branch, folder `/docs`. Enable in repo **Settings → Pages → Source:** Deploy from a branch, folder **/docs**.
- **Docs:** `documents/HOW_TO_RUN_LOCALLY.md` — under Building an installer, added link to the download page and note on enabling GitHub Pages. `documents/RELEASE.md` — added "Where to get installers (end users)" pointing to the download page and stable asset names.

## 2026-02-03 (One-click installer build)

- **tauri-app:** Set `beforeBuildCommand` in `tauri.conf.json` to `cd src-ui && npm run build` so `cargo tauri build` from `tauri-app` builds the frontend automatically (B2: assumes `npm install` already run once in `tauri-app/src-ui`). CI unchanged (still runs frontend install/build explicitly).
- **Docs:** `documents/HOW_TO_RUN_LOCALLY.md` — added **Building an installer** with Option A (CI: tag or workflow_dispatch → download artifact/Release, then run `.exe`/`.dmg`/`.deb`) and Option B (local: one-time `npm install` in `tauri-app/src-ui`, then `cd tauri-app && cargo tauri build`; installer under `tauri-app/target/release/bundle/` or workspace `target/release/bundle/`).
- **Scripts:** Added `scripts/build-installer.ps1` (Windows) and `scripts/build-installer.sh` (macOS/Linux) to install frontend deps, run `cargo tauri build` from `tauri-app`, and open the bundle folder. Run from repo root.

## 2026-02-03 (Build and release from workflow_dispatch)

- **workflow_dispatch:** Added optional input `release_version` (e.g. `0.0.1`). When set, the release job runs and creates a draft GitHub Release with tag `v<release_version>` and all installer artifacts. When empty, only build jobs run (no release). Documented in `documents/RELEASE.md`.

## 2026-02-03 (Release 0.0.1 and incremental versioning)

- **Version:** Set workspace and app version to **0.0.1** for first release (root `Cargo.toml`, `tauri-app/tauri.conf.json`).
- **Workflow:** Release step moved to a dedicated `release` job that runs after all `build` matrix jobs. It downloads all installer artifacts (Windows, Ubuntu, macOS), then creates a single draft GitHub Release with all three installers attached (no per-job releases).
- **Docs:** Added `documents/RELEASE.md` with version scheme (0.0.x incremental), where version is defined, and step-by-step instructions to publish a release and to cut the first release (v0.0.1). Incremental checklist for future 0.0.2, 0.0.3, etc.

## 2026-02-03 (CI: Windows .ico + Rust warnings)

- **Windows bundle:** CI failed with "Couldn't find a .ico icon" because the Tauri bundler (WiX) runs with cwd at repo root while icons live in tauri-app/icons/. Added workflow step "Ensure icons at repo root (Windows bundler cwd)" (Windows only): copy tauri-app/icons/* to repo root `icons/` so `icons/icon.ico` exists from cwd.
- **Rust warnings (warning-clean build):** abigail-core keyring.rs: removed unused `base64` imports. abigail-skills manifest.rs: removed unused `ResourceLimits` import. abigail-skills executor.rs: removed unused `Skill` import, prefixed `tool_name` with `_`. tauri-app lib.rs: added `#[allow(dead_code)]` on `event_bus` (kept for future skill-event UI wiring).

## 2026-02-03 (Build-release remediation plan implementation)

- **Rust:** `abigail-core` keyring.rs already uses `let _ = LocalFree(...)` on both Windows DPAPI paths (lines 119, 152); no code change. Cargo.lock: workflow already runs `cargo generate-lockfile` in CI. For full reproducibility, generate and commit `Cargo.lock` at repo root when Rust/Docker is available (`cargo generate-lockfile`).
- **Tauri bundle:** tauri.conf.json already has `identifier: "com.abigail"` and `bundle.icon` including `icons/icon.ico`; icons exist under tauri-app/icons. Workflow step "Generate app icons" runs `tauri icon icons/icon.png -o icons` in CI.
- **Workflow:** Pinned `tauri-apps/tauri-action` to SHA `063c0231f444e55760d98acb9c469b994269d4a5` (reproducible builds). Node already pinned to `20`. Ubuntu step already includes libwebkit2gtk-4.1-dev, libappindicator3-dev, librsvg2-dev, patchelf, libgtk-3-dev; matches Tauri 2 Linux requirements.
- **Frontend:** `npm run build` in tauri-app/src-ui succeeds (tsc && vite build); no TS/lint fixes required.
- **Verification:** After push or workflow_dispatch, confirm all three matrix jobs (windows-latest, macos-latest, ubuntu-22.04) pass and artifacts `abigail-installer-<platform>` are uploaded.

## 2026-02-03 (Troubleshooting resume)

- **CI failures addressed in repo:** (1) `ao_core::EmailConfig` — `EmailConfig` is defined in `abigail-core/src/config.rs` and re-exported in `abigail-core/src/lib.rs` via `pub use config::{AppConfig, EmailConfig}`; abigail-birth uses `ao_core::EmailConfig` and should resolve. (2) `abigail-skills` — `SkillId` has `impl Display` in `manifest.rs`; permission parsing uses `s.permission.as_table()` (returns `Option<&Map>`) not `Value::Table` pattern. If CI still fails, ensure the commit that added these fixes is the one being built.
- **Workflow:** Release step now sets `tag_name: ${{ github.ref_name }}` so the draft release is explicitly tied to the pushed tag.

## 2026-02-03 (Stable Windows release pipeline)

- **Trigger:** Workflow runs on version tags (`v*`, e.g. `v0.1.0`) and `workflow_dispatch`. No longer runs on every push to master.
- **Release:** Added `softprops/action-gh-release@v1` to create a draft GitHub Release and attach installer artifacts. Requires `permissions: contents: write`. Release step runs only when `github.ref` is a tag (`refs/tags/`).
- **Artifacts:** Upload path is now platform-specific: Windows `target/release/bundle/nsis/*.exe`, Linux `target/release/bundle/deb/*.deb`, macOS `target/release/bundle/dmg/*.dmg`.
- **Rust toolchain:** Removed hardcoded `targets = ["x86_64-pc-windows-msvc"]` from `rust-toolchain.toml`; each matrix job sets `target` via `dtolnay/rust-toolchain` (e.g. `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`) to avoid cross-platform toolchain conflicts.
- **Tauri action:** Switched to `tauri-apps/tauri-action@v0` with `GITHUB_TOKEN` for release uploads.
- **To publish a release:** `git tag v0.1.0 && git push origin v0.1.0`. Then open the draft release on the repo Releases page and publish.

## 2026-02-03 (MVP build: Windows only, senses out of scope)

- **Workflow:** Build only Windows (`windows-latest`); macOS and Ubuntu removed from matrix for initial MVP focus.
- **Senses/SMTP:** Not required for MVP. Removed `abigail-skills` and `skill-proton-mail` from workspace members and from `tauri-app` and `abigail-birth` deps. Birth `configure_email` now stores email config without IMAP validation. Proton Mail skill registration removed from app startup; registry starts empty. Email/senses can be re-added in a later phase.

## 2026-02-03 (Build-release remediation)

- **Rust:** Fixed abigail-core keyring.rs `LocalFree` unused result (use `let _ = LocalFree(...)` on Windows DPAPI paths). Workflow now runs `cargo generate-lockfile` after Setup Rust so CI uses a consistent lockfile for the run (Cargo.lock not yet committed; generate when Rust/Docker available and commit for full reproducibility).
- **Tauri:** identifier changed from `com.abigail.app` to `com.abigail` (avoid macOS .app conflict). Added `tauri-app/icons/icon.png` (minimal PNG) and set `bundle.icon` to `["icons/icon.png"]`.
- **Workflow:** Pinned tauri-action to SHA `063c0231f444e55760d98acb9c469b994269d4a5` (dev). Pinned Node to `20`. Ubuntu step: added `libgtk-3-dev`. Added step "Generate Cargo.lock" before Rust cache.

## 2025-02-02 (CI)

- **Added GitHub Actions workflow `build-release.yml`.** Builds Tauri installers (Abigail) on push to `master`; artifacts only (no GitHub Release). Matrix: windows-latest, macos-latest, ubuntu-22.04. Steps: checkout, Linux deps (Ubuntu), Node + npm cache, Rust + rust-cache, frontend install/build in `tauri-app/src-ui`, tauri-apps/tauri-action with projectPath `tauri-app`, upload-artifact from `target/release/bundle/`.

## 2025-02-02

- **Initial Abigail MVP scaffold.** Workspace: Rust with crates abigail-core, abigail-memory, abigail-llm, abigail-router, abigail-birth, abigail-skills, tauri-app. Constitutional docs in templates/ and embedded in tauri-app for init_soul. Documents folder and example.env added.

- **Plugin & Skills abstraction layer (plan Phases 1–6).**
  - **abigail-skills crate:** manifest (skill.toml parsing), registry, executor, sandbox (permission checks + audit log), capability traits (llm, email, audio, video, memory, agent, mcp), channel (triggers, EventBus), prelude, core Skill trait.
  - **skill-proton-mail:** New workspace member under `skills/skill-proton-mail`. Implements Skill and EmailTransportCapability; wraps abigail-skills IMAP (fetch_emails); send_email/move/delete stubbed. Tools: fetch_emails, send_email, classify_importance, create_filter. Emits `email_received` event after fetch when event_sender is set.
  - **tauri-app:** AppState extended with `registry: Arc<SkillRegistry>`, `executor: Arc<SkillExecutor>`, `event_bus: Arc<EventBus>`. Proton Mail skill registered at startup (initialized when email config + decrypted password present). Commands: `list_skills`, `list_tools`, `execute_tool`, `list_discovered_skills`. Event bus subscription forwards `skill-event` to frontend.
  - **Sandbox:** `check_permission` enforces Network (domain allowlist), FileSystem (path allowlist), Memory (namespace). Executor builds sandbox from manifest and checks tool’s required_permissions (e.g. Network) before `execute_tool`; returns PermissionDenied when not granted.
  - **Discovery:** `SkillRegistry::discover(paths)` scans directories for `skill.toml` (metadata only); `list_discovered_skills` uses `data_dir/skills`. Loading remains explicit (compiled-in skills only).
