# Environment Updates

Dated log of environment, dependency, CI, container, or infrastructure changes. No sensitive data.

## 2026-02-03 (MVP build: Windows only, senses out of scope)

- **Workflow:** Build only Windows (`windows-latest`); macOS and Ubuntu removed from matrix for initial MVP focus.
- **Senses/SMTP:** Not required for MVP. Removed `abby-senses` and `skill-proton-mail` from workspace members and from `tauri-app` and `abby-birth` deps. Birth `configure_email` now stores email config without IMAP validation. Proton Mail skill registration removed from app startup; registry starts empty. Email/senses can be re-added in a later phase.

## 2026-02-03 (Build-release remediation)

- **Rust:** Fixed abby-core keyring.rs `LocalFree` unused result (use `let _ = LocalFree(...)` on Windows DPAPI paths). Workflow now runs `cargo generate-lockfile` after Setup Rust so CI uses a consistent lockfile for the run (Cargo.lock not yet committed; generate when Rust/Docker available and commit for full reproducibility).
- **Tauri:** identifier changed from `com.abby.app` to `com.abby` (avoid macOS .app conflict). Added `tauri-app/icons/icon.png` (minimal PNG) and set `bundle.icon` to `["icons/icon.png"]`.
- **Workflow:** Pinned tauri-action to SHA `063c0231f444e55760d98acb9c469b994269d4a5` (dev). Pinned Node to `20`. Ubuntu step: added `libgtk-3-dev`. Added step "Generate Cargo.lock" before Rust cache.

## 2025-02-02 (CI)

- **Added GitHub Actions workflow `build-release.yml`.** Builds Tauri installers (Abby) on push to `master`; artifacts only (no GitHub Release). Matrix: windows-latest, macos-latest, ubuntu-22.04. Steps: checkout, Linux deps (Ubuntu), Node + npm cache, Rust + rust-cache, frontend install/build in `tauri-app/src-ui`, tauri-apps/tauri-action with projectPath `tauri-app`, upload-artifact from `target/release/bundle/`.

## 2025-02-02

- **Initial Abby MVP scaffold.** Workspace: Rust with crates abby-core, abby-memory, abby-llm, abby-router, abby-birth, abby-senses, tauri-app. Constitutional docs in templates/ and embedded in tauri-app for init_soul. Documents folder and example.env added.

- **Plugin & Skills abstraction layer (plan Phases 1–6).**
  - **abby-skills crate:** manifest (skill.toml parsing), registry, executor, sandbox (permission checks + audit log), capability traits (llm, email, audio, video, memory, agent, mcp), channel (triggers, EventBus), prelude, core Skill trait.
  - **skill-proton-mail:** New workspace member under `skills/skill-proton-mail`. Implements Skill and EmailTransportCapability; wraps abby-senses IMAP (fetch_emails); send_email/move/delete stubbed. Tools: fetch_emails, send_email, classify_importance, create_filter. Emits `email_received` event after fetch when event_sender is set.
  - **tauri-app:** AppState extended with `registry: Arc<SkillRegistry>`, `executor: Arc<SkillExecutor>`, `event_bus: Arc<EventBus>`. Proton Mail skill registered at startup (initialized when email config + decrypted password present). Commands: `list_skills`, `list_tools`, `execute_tool`, `list_discovered_skills`. Event bus subscription forwards `skill-event` to frontend.
  - **Sandbox:** `check_permission` enforces Network (domain allowlist), FileSystem (path allowlist), Memory (namespace). Executor builds sandbox from manifest and checks tool’s required_permissions (e.g. Network) before `execute_tool`; returns PermissionDenied when not granted.
  - **Discovery:** `SkillRegistry::discover(paths)` scans directories for `skill.toml` (metadata only); `list_discovered_skills` uses `data_dir/skills`. Loading remains explicit (compiled-in skills only).
