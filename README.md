# Abigail - Your Personal AI Family Companion

**The private coordinator that manages your family's personal AI Entities.**

Abigail is the smart, private manager that lives in your home. You, the mentor or family head, create and customize individual **Entities** - each one a unique AI companion tailored to a person or purpose.

You first talk to **Abigail** to get the application ready. After a welcoming splash screen, a packaged local model loads and generates a test reply before chat becomes available. The first conversation works offline after installation, with no separate Ollama install or model download.

Abigail guides you through connecting Anthropic (Claude) or OpenAI when you want a more capable model. Enter the API key in **Connect a model**, select a model, and choose **Test and connect**. Abigail saves the connection encrypted only after that model returns a real reply. The identity and conversation continue through the change and across app restarts. API access may require separate billing from a consumer chat subscription.

The local assistant has a narrow setup role. Cloud replies send recent conversation context to the provider you select. **Continue locally** returns the conversation to the packaged model. Family accounts, age-based permissions, and a redesigned conversational Entity birth process remain future work.

### Quick Start

1. Download and run the full Windows x64 installer.
2. Launch **Abigail** and let the local model load. Loading can take longer on modest computers.
3. Ask Abigail about setting up API access, or keep talking offline.
4. Use **Connect a model** to test and save your chosen API connection.
5. Existing Entity creation and chat remain available below the setup conversation; Abigail stays open alongside them.

The Windows package includes Ollama and Qwen3.5-0.8B. The staged CPU runtime and model add about 1.11 GB before installer compression, in addition to the application and offline WebView2 installer. See [initial setup and verification](docs/INITIAL_SETUP.md) for packaging, licenses, and development instructions.

## Current Dev Note

- Abigail is still in a dev-first phase. A clean working dev instance matters more than cross-version compatibility right now.
- Legacy migration and upgrade preservation are not current product promises. Remove or replace stale compatibility paths when they get in the way of the active Hive-first design.
- The implementation uses two desktop app roots: `hive-app` for the Abigail coordinator and `entity-runtime-app` for Entity chat. The family-facing installer must still expose one `Abigail` app icon and start the internal pieces automatically.
- `beta` is the permanent UAT branch. Iterative work lands there first and produces tagged beta installer prereleases; `main` receives only promoted stable changes.
- Default local builds remain unsigned and updater-free. The optional [SSL.com signed Windows lane](docs/WINDOWS_SIGNING.md) signs and verifies the full offline installer and its internal Abigail programs before release.
- Repeatable release automation is documented in [`docs/RELEASE_RUNBOOK.md`](docs/RELEASE_RUNBOOK.md). The active full installer release lane currently builds the Windows one-step installer; Apple/macOS builds are temporarily removed from the matrix.
- UI and UX work must follow the Abigail design system in [`docs/design/README.md`](docs/design/README.md).

## Dev Start

- Stage the offline bundle with `pwsh ./scripts/stage_offline_bootstrap.ps1`, then use the split stack launcher below. The legacy root Tauri app is not the current setup entry point.
- If you only need the Hive frontend shell, run `npm run dev` in `hive-app/src-ui`.
- If you only need the Entity Runtime frontend shell, run `npm run dev` in `entity-runtime-app/src-ui`.
- The Tauri watcher now ignores frontend dependency churn through `.taurignore`, so Vite temp files should not retrigger Rust rebuilds during normal dev.
- On Windows machines with Application Control enabled, `cargo build` and `cargo tauri dev` can still fail with `os error 4551` when Cargo tries to execute generated build-script binaries. That is an OS policy blocker, not an Abigail source-code failure. Use a build-allowed environment to launch the desktop shell in that case.

## Split Stack Local Dev

- Use `pwsh ./scripts/dev/launch_split_stack.ps1` to build and launch the Hive daemon, Entity Runtime daemon, and the split desktop shells from one command.
- Use `pwsh ./scripts/stage_split_installer_resources.ps1` when validating the Windows installer payload locally; it stages the internal split binaries that the family installer bundles behind the single Abigail app icon.
- Those scripts standardize `CARGO_TARGET_DIR` to `%LOCALAPPDATA%\Abigail\cargo-target` on Windows so allow-listing can target one stable developer build path instead of repo-local `target\...`.
- If Windows policy blocks desktop-shell builds, use `pwsh ./scripts/diagnose_windows_build_policy.ps1` for a JSON diagnostic summary. The old unauthenticated browser harness is not a supported fallback.
- For isolated setup UI verification after building both daemons, run `python scripts/test_initial_setup.py --ui`. Its browser server uses only a disposable test profile; never expose personal credentials through Vite.
- Session details, pids, and logs are written to `target/manual-test/stability-reset/session.json`. Shut them down with `pwsh ./scripts/dev/stop_split_stack.ps1`.

## Local Memory Model

- The `hive-daemon` process owns and opens the shared `memory.db`. Abigail setup runs inside that process under the immortal coordinator identity. Family Entity processes use authenticated, scoped persistence requests instead of opening the locked file themselves.
- Shared orchestration state lives in the `abigail/hive` Surreal namespace/database pair.
- Per-Entity state is isolated into `abigail/entity_<uuid>` databases inside the same local store.
- Research-era SQLite artifacts such as `abigail_seed.db`, `abigail_memory.db`, `jobs.db`, `calendar.db`, and `kb.db` are legacy-only and should not be treated as active runtime stores.
- Browser, mentor-monitor, Id, Superego, and memory enrichment flows stay out-of-band so the family chat path remains responsive.

### For the Curious: Where Abigail Came From
Abigail began as part of a larger research vision for trustworthy, decentralized AI. We took the best parts of that vision and made them simple, safe, and useful for real families.

See [documents/DECENTRALIZED_TRUST_PAPER_ALIGNMENT.md](documents/DECENTRALIZED_TRUST_PAPER_ALIGNMENT.md).
