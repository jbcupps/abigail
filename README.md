# Abigail - Your Personal AI Family Companion

**The private coordinator that manages your family's personal AI Entities.**

Abigail is the smart, private manager that lives in your home. You, the mentor or family head, create and customize individual **Entities** - each one a unique AI companion tailored to a person or purpose.

Your family talks directly to these Entities. Abigail coordinates everything behind the scenes: memory, skills, security, and growth.
Every install also provisions an immortal local coordinator identity called `Abigail Hive` that owns the shared `memory.db`, stays visible while an Entity is open, and centralizes provider/model management for the whole home.
That Hive-owned store now runs on embedded SurrealDB, giving Abigail one local-first memory substrate for document records, graph links, semantic archives, and queue state without requiring Docker or an external database.

### Why Families Love Abigail
- **Complete privacy** - everything stays on your computer. No accounts with big tech companies.
- **One shared local memory core** - Abigail Hive owns a single embedded SurrealDB store for shared coordination, memory, archives, and queue state.
- **Real power when you want it** - connect any Entity to the strongest cloud models with one click. You are never locked to one provider.
- **Hive-owned model control** - provider and model changes happen in Abigail Hive, not inside each individual Entity chat.
- **Grows with your family** - teach Entities new skills, memories, and preferences over time.
- **Simple for everyone** - kids, parents, and partners just chat naturally while you control the advanced settings.
- **Handles real-world sites safely** - for authenticated flows like webmail, Abigail uses Browser skill fallback instead of fragile IMAP/SMTP plumbing.

### Quick Start (5 minutes)
1. Download and run the Windows installer
2. Launch Abigail from the installed app icon
3. Create your first Entity in Abigail Hive
4. Configure local or cloud models in the Hive sidebar if you want extra capability
5. Let your family chat with the selected Entity while the Hive stays open beside it

No accounts. No data sharing. Just your family and the Entities you control.

## Current Dev Note

- Abigail is still in a dev-first phase. A clean working dev instance matters more than cross-version compatibility right now.
- Legacy migration and upgrade preservation are not current product promises. Remove or replace stale compatibility paths when they get in the way of the active Hive-first design.
- The implementation uses two desktop app roots: `Abigail Hive` for control-plane/admin work and `Abigail Entity Runtime` for chat/runtime work. The family-facing installer must still expose one `Abigail` app icon and start the internal pieces automatically.
- `beta` is the permanent UAT branch. Iterative work lands there first and produces tagged beta installer prereleases; `main` receives only promoted stable changes.
- Default local builds and installer validation are intentionally unsigned and updater-free during stabilization. Final OV signing happens later on the dedicated release-signing system.
- Repeatable release automation is documented in [`docs/RELEASE_RUNBOOK.md`](docs/RELEASE_RUNBOOK.md). The active full installer release lane currently builds the Windows one-step installer; Apple/macOS builds are temporarily removed from the matrix.
- UI and UX work must follow the Abigail design system in [`docs/design/README.md`](docs/design/README.md).

## Dev Start

- From the repo root, start the desktop app with `cargo tauri dev`.
- If you only need the Hive frontend shell, run `npm run dev` in `hive-app/src-ui`.
- If you only need the Entity Runtime frontend shell, run `npm run dev` in `entity-runtime-app/src-ui`.
- The Tauri watcher now ignores frontend dependency churn through `.taurignore`, so Vite temp files should not retrigger Rust rebuilds during normal dev.
- On Windows machines with Application Control enabled, `cargo build` and `cargo tauri dev` can still fail with `os error 4551` when Cargo tries to execute generated build-script binaries. That is an OS policy blocker, not an Abigail source-code failure. Use a build-allowed environment to launch the desktop shell in that case.

## Split Stack Local Dev

- Use `pwsh ./scripts/dev/launch_split_stack.ps1` to build and launch the Hive daemon, Entity Runtime daemon, and the split desktop shells from one command.
- Use `pwsh ./scripts/stage_split_installer_resources.ps1` when validating the Windows installer payload locally; it stages the internal split binaries that the family installer bundles behind the single Abigail app icon.
- Those scripts standardize `CARGO_TARGET_DIR` to `%LOCALAPPDATA%\Abigail\cargo-target` on Windows so allow-listing can target one stable developer build path instead of repo-local `target\...`.
- If Windows policy still blocks desktop-shell builds, run `pwsh ./scripts/diagnose_windows_build_policy.ps1` for a JSON diagnostic summary and use `pwsh ./scripts/dev/launch_split_stack.ps1` to fall back to the browser harness automatically.
- The browser fallback lives at `dev-harness/` and is served by `node ./scripts/dev/run_browser_harness.mjs`; it talks directly to the local Hive and Runtime daemon APIs.
- Session details, pids, and logs are written to `target/manual-test/stability-reset/session.json`. Shut them down with `pwsh ./scripts/dev/stop_split_stack.ps1`.

## Local Memory Model

- `Abigail Hive` owns the shared persistence root and opens the local `memory.db` store before user-facing Entities start.
- Shared orchestration state lives in the `abigail/hive` Surreal namespace/database pair.
- Per-Entity state is isolated into `abigail/entity_<uuid>` databases inside the same local store.
- Research-era SQLite artifacts such as `abigail_seed.db`, `abigail_memory.db`, `jobs.db`, `calendar.db`, and `kb.db` are legacy-only and should not be treated as active runtime stores.
- Browser, mentor-monitor, Id, Superego, and memory enrichment flows stay out-of-band so the family chat path remains responsive.

### For the Curious: Where Abigail Came From
Abigail began as part of a larger research vision for trustworthy, decentralized AI. We took the best parts of that vision and made them simple, safe, and useful for real families.

See [documents/DECENTRALIZED_TRUST_PAPER_ALIGNMENT.md](documents/DECENTRALIZED_TRUST_PAPER_ALIGNMENT.md).
