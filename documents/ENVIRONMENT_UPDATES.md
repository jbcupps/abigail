# Environment Updates

Dated log of environment, dependency, CI, container, or infrastructure changes. No sensitive data.

## 2026-02-03 (Release 0.0.1 and incremental versioning)

- **Version:** Set workspace and app version to **0.0.1** for first release (root `Cargo.toml`, `tauri-app/tauri.conf.json`).
- **Workflow:** Release step moved to a dedicated `release` job that runs after all `build` matrix jobs. It downloads all installer artifacts (Windows, Ubuntu, macOS), then creates a single draft GitHub Release with all three installers attached (no per-job releases).
- **Docs:** Added `documents/RELEASE.md` with version scheme (0.0.x incremental), where version is defined, and step-by-step instructions to publish a release and to cut the first release (v0.0.1). Incremental checklist for future 0.0.2, 0.0.3, etc.

## 2026-02-03 (CI: Windows .ico + Rust warnings)

- **Windows bundle:** CI failed with "Couldn't find a .ico icon" because the Tauri bundler (WiX) runs with cwd at repo root while icons live in tauri-app/icons/. Added workflow step "Ensure icons at repo root (Windows bundler cwd)" (Windows only): copy tauri-app/icons/* to repo root `icons/` so `icons/icon.ico` exists from cwd.
- **Rust warnings (warning-clean build):** abby-core keyring.rs: removed unused `base64` imports. abby-skills manifest.rs: removed unused `ResourceLimits` import. abby-skills executor.rs: removed unused `Skill` import, prefixed `tool_name` with `_`. tauri-app lib.rs: added `#[allow(dead_code)]` on `event_bus` (kept for future skill-event UI wiring).

## 2026-02-03 (Build-release remediation plan implementation)

- **Rust:** `abby-core` keyring.rs already uses `let _ = LocalFree(...)` on both Windows DPAPI paths (lines 119, 152); no code change. Cargo.lock: workflow already runs `cargo generate-lockfile` in CI. For full reproducibility, generate and commit `Cargo.lock` at repo root when Rust/Docker is available (`cargo generate-lockfile`).
- **Tauri bundle:** tauri.conf.json already has `identifier: "com.abby"` and `bundle.icon` including `icons/icon.ico`; icons exist under tauri-app/icons. Workflow step "Generate app icons" runs `tauri icon icons/icon.png -o icons` in CI.
- **Workflow:** Pinned `tauri-apps/tauri-action` to SHA `063c0231f444e55760d98acb9c469b994269d4a5` (reproducible builds). Node already pinned to `20`. Ubuntu step already includes libwebkit2gtk-4.1-dev, libappindicator3-dev, librsvg2-dev, patchelf, libgtk-3-dev; matches Tauri 2 Linux requirements.
- **Frontend:** `npm run build` in tauri-app/src-ui succeeds (tsc && vite build); no TS/lint fixes required.
- **Verification:** After push or workflow_dispatch, confirm all three matrix jobs (windows-latest, macos-latest, ubuntu-22.04) pass and artifacts `abby-installer-<platform>` are uploaded.

## 2026-02-03 (Troubleshooting resume)

- **CI failures addressed in repo:** (1) `abby_core::EmailConfig` — `EmailConfig` is defined in `abby-core/src/config.rs` and re-exported in `abby-core/src/lib.rs` via `pub use config::{AppConfig, EmailConfig}`; abby-birth uses `abby_core::EmailConfig` and should resolve. (2) `abby-skills` — `SkillId` has `impl Display` in `manifest.rs`; permission parsing uses `s.permission.as_table()` (returns `Option<&Map>`) not `Value::Table` pattern. If CI still fails, ensure the commit that added these fixes is the one being built.
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
