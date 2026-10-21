# Abigail

**A private, local-first framework for creating and hosting persistent AI Entities.**

Abigail is not a single assistant. It is the framework a mentor or family head uses to create, birth, sign, and host many persistent AI **Entities**, each with its own name, purpose, soul, keys, memory, and conversation history. The family talks directly to those Entities; Abigail coordinates memory, skills, security, and model access behind them.

Every install includes `Abigail Hive`, an immortal local coordinator Entity. Hive owns the shared `memory.db`, manages providers and models for the whole home, and stays open while any Entity is in use.

## How Abigail Fits Together

| Part | What it does |
| --- | --- |
| **Hive** (`hive-app` + `hive-daemon`) | Control plane. Creates Entities, runs the birth rite, signs each Entity's identity with the Hive master key, manages providers and secrets, supervises Entity runtimes, and is the only process that opens the embedded store. |
| **Entity Runtime** (`entity-runtime-app` + `entity-daemon`) | One `entity-daemon` per open Entity. It runs chat, routing, skills, memory, and Superego checks for that Entity, and opens a chat window while Hive stays available. |
| **Shared memory** (`memory.db`) | One embedded SurrealDB store owned by Hive. Hive coordination lives in database `hive`; each Entity gets its own database `entity_<UUID>` (hyphens become underscores) in namespace `abigail`. Runtimes reach only their own database through a Hive-issued lease over local HTTP. |

The family installs and launches one `Abigail` app. Packaging hides the split binaries and starts Hive, the Entity Runtime, and both daemons automatically.

### Entity lifecycle

1. **Create.** Hive registers a new Entity UUID with its own identity directory.
2. **Birth.** The Soul Forge rite asks three ethical trials, or a quickstart path picks a balanced soul. The resulting Triangle Ethic weights shape the Entity's soul documents.
3. **Sign.** Hive generates the Entity's Ed25519 key, signs its constitutional documents, countersigns its public key with the Hive master key, and issues a signed birth certificate.
4. **Run.** Opening the Entity starts its `entity-daemon`. Loading fails closed if the signature chain or constitution does not verify.
5. **Persist.** Conversations, memories, and jobs stay in that Entity's database, so closing and reopening it restores its history.

### Framework crates

| Crate | Role | Used by the active runtime |
| --- | --- | --- |
| `abigail-identity` | Entity registry, Hive master key, birth signing, and fail-closed verification on load | `hive-daemon`, `entity-daemon` |
| `soul-forge` | Ethical-trial scenarios, Triangle Ethic weights, soul hash, and sigil | `hive-daemon` birth rite |
| `abigail-birth` | Conversational birth orchestrator and genesis paths | Library only; not yet wired into Hive |
| `abigail-soul-crystallization` | LLM-guided soul crystallization (quick, conversation, deep dive) | Library only, via `abigail-birth` |
| `abigail-memory` | Conversation turns, memories, archives, and backup import | `entity-daemon` |
| `abigail-persistence` | Embedded SurrealDB handle, Hive/Entity scopes, and the lease-scoped remote transport | Both daemons |
| `abigail-superego` | Ethical evaluation and output gating | `entity-daemon` |
| `abigail-router` | Id/Ego routing, agentic runs, subagents, and the mentor chat monitor | `entity-daemon` |
| `abigail-skills` | Skill registry, execution, signed allowlists, and confirmation policy | Both daemons |
| `abigail-hive` | Provider construction, secret resolution, and installed-CLI detection | Both daemons |

Supporting crates include `abigail-core` (config, vault, keyring, constitutional verification), `abigail-capabilities`, `abigail-runtime`, `abigail-queue`, `abigail-streaming`, `entity-chat`, and the DTO-only contracts `hive-core` and `entity-core`. Skills live under `skills/`.

## Why Families Love Abigail

- **Local ownership.** Identities and conversation history stay on your computer. A connected cloud provider receives the messages and context it processes; a connected local model can process them on this computer.
- **Many Entities, one memory core.** Each Entity keeps its own history inside the single Hive-owned store, and one Entity cannot read another's records by accident.
- **Any provider you like.** Connect Entities to strong cloud models from any provider, a local server, or an installed official CLI. You are never locked to one vendor.
- **Hive-owned model control.** Provider and model changes happen in Abigail Hive, not inside each Entity chat.
- **Grows with your family.** Teach Entities new skills, memories, and preferences over time.
- **Simple for everyone.** Kids, parents, and partners just chat while you control the advanced settings.
- **Handles real-world sites safely.** For authenticated flows like webmail, Abigail uses Browser skill fallback instead of fragile IMAP/SMTP plumbing.

## Quick Start

1. Download and run the Windows installer.
2. Launch Abigail from the installed app icon.
3. Connect a model in Abigail Hive: a loaded local Ollama/LM Studio server, a cloud provider key, or an installed official Claude, Codex, or Grok CLI with your existing account.
4. Create your first Entity with a name and purpose, then complete its birth.
5. Open the Entity and chat while Hive stays available beside it. Reopen the Entity later to continue its saved conversation.

Hive checks an installed CLI connection with a real model response before saving it. Installed Codex and Grok cards let you check an existing connection even when sign-in cannot be confirmed during discovery; discovery itself never sends a model request. Gemini CLI is not supported in this MVP. A desktop app alone does not establish a working CLI connection.

Local storage belongs to you. Cloud providers and installed CLIs use their own accounts and processing policies. The official CLI keeps control of account sign-in and tokens; Abigail does not extract or forward account OAuth tokens. The MVP requires a connected, working model; it does not bundle model weights or pretend a placeholder is local intelligence.

## Unsigned Windows MVP

- Build the self-contained installer with `pwsh ./scripts/build-mvp-windows.ps1 -VerifyInstall`.
- The installer is written to `dist/windows-mvp/Abigail-windows-x64-setup.exe`. It bundles Hive, Entity Runtime, and both daemons with embedded frontend assets; no development server, SSL.com certificate, or updater key is required.
- See [the MVP installation and acceptance guide](docs/MVP_WINDOWS.md) for setup, isolated validation, and current scope.
- Installed Codex `0.153.4` passed real account inference, live streaming, conversation context, and Entity/Hive restart acceptance without an injected API key. The final unsigned installer and eight-stage desktop workflow are verified. Grok Build `0.2.93` and Claude passed failed-connection checks without saving a default; their real inference awaits user sign-in with `grok login` and `claude auth login`.
- Model setup applies to the next conversation turn. Installed CLI connections are chat-only: native tools and extensions must be disabled or the connection is rejected. Tools requiring mentor confirmation cannot silently execute from chat or background jobs.
- Claude Code integration uses the unmodified official CLI and each user's own account, subject to [Anthropic's product-use conditions](https://code.claude.com/docs/en/legal-and-compliance#can-customers-offer-claude-code-in-their-products). The current Codex account path is for local or open-source use; commercial or hosted distribution needs the applicable approved [Sign in with ChatGPT integration](https://learn.chatgpt.com/docs/app-server#auth-endpoints).

## Development

- Abigail is in a dev-first phase. A clean working dev instance matters more than cross-version compatibility, and stale compatibility paths are removed when they conflict with the Hive-first design.
- `beta` is the permanent UAT branch. Iterative work lands there first and produces tagged beta installer prereleases; `main` receives only promoted stable changes.
- Default local builds and installer validation are unsigned and updater-free during stabilization. Automated SSL.com cloud releases use the [Windows cloud signing guide](docs/windows-cloud-signing.md); local certificate-store signing uses the [Windows signing guide](docs/WINDOWS_SSL_COM_SIGNING.md). Release automation is documented in [`docs/RELEASE_RUNBOOK.md`](docs/RELEASE_RUNBOOK.md).
- UI and UX work follows the design system in [`docs/design/README.md`](docs/design/README.md).
- Contributor setup and checks are in [`CONTRIBUTING.md`](CONTRIBUTING.md); daemon-only and Docker workflows are in [`documents/HOW_TO_RUN_LOCALLY.md`](documents/HOW_TO_RUN_LOCALLY.md).

### Run the desktop apps

Hive looks for `hive-daemon`, `entity-daemon`, and the Entity Runtime app next to its own executable, so build them first:

```bash
npm ci --prefix hive-app/src-ui && npm ci --prefix entity-runtime-app/src-ui
cargo build -p hive-daemon -p entity-daemon -p abigail-entity-runtime-app
npm run dev --prefix entity-runtime-app/src-ui &   # debug Runtime windows load their UI from this server
cd hive-app && cargo tauri dev
```

- On Windows, `pwsh ./scripts/dev/launch_split_stack.ps1` builds and launches both daemons and both desktop shells in one step, and falls back to the browser harness in `dev-harness/` when policy blocks desktop builds. Stop it with `pwsh ./scripts/dev/stop_split_stack.ps1`.
- For frontend-only work, run `npm run dev` in `hive-app/src-ui` or `entity-runtime-app/src-ui`.
- `pwsh ./scripts/stage_split_installer_resources.ps1` stages the internal binaries the family installer bundles behind the single Abigail icon.
- Windows Application Control can block Cargo build scripts with `os error 4551`. That is an OS policy blocker, not a source failure; `pwsh ./scripts/diagnose_windows_build_policy.ps1` prints a JSON diagnostic.

### Local memory model

- Hive opens the shared `memory.db` store before any family Entity starts. Legacy SQLite files (`abigail_seed.db`, `abigail_memory.db`, `jobs.db`, `calendar.db`, `kb.db`) are first-launch migration inputs only, never active stores.
- Browser, mentor-monitor, Id, Superego, and memory enrichment flows stay out-of-band so the family chat path remains responsive.

### Legacy single-agent app

The original single-agent Tauri app (`abigail-app`) is retired. It now lives in [`legacy/tauri-app/`](legacy/README.md) as its own Cargo workspace, outside the framework build, installers, and CI.

## Where Abigail Came From

Abigail began as part of a larger research vision for trustworthy, decentralized AI. We took the best parts of that vision and made them simple, safe, and useful for real families. See [documents/DECENTRALIZED_TRUST_PAPER_ALIGNMENT.md](documents/DECENTRALIZED_TRUST_PAPER_ALIGNMENT.md).
