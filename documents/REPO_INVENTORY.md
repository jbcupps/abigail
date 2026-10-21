# Repo Inventory Matrix

This matrix describes the live repo surface of the Abigail framework. It is
anchored to the root `Cargo.toml` workspace, `.github/workflows/ci.yml`,
`.github/workflows/release.yml`, and `documents/HOW_TO_RUN_LOCALLY.md`.
"Used by" lists direct dependents inside the framework workspace.

## Top-Level Directories

| Path | Role | Coverage / Validation | Disposition |
| --- | --- | --- | --- |
| `.claude/` | Agent tooling only | Local only | keep |
| `.github/` | CI, release, templates | GitHub Actions gate + release workflows | keep |
| `crates/` | Framework crates, daemons, and CLIs | `cargo check`, workspace tests | keep |
| `hive-app/` | Hive desktop shell (`abigail-hive-app`); the installed `Abigail` app | `cargo check`, frontend build/tests | keep |
| `entity-runtime-app/` | Entity Runtime desktop shell (`abigail-entity-runtime-app`) | `cargo check`, frontend build/tests | keep |
| `dev-harness/` | Browser fallback UI over the daemon APIs | manual | keep |
| `docker/` | Containerized build/test | docker compose docs | keep |
| `docs/` | Architecture, MVP, release, signing, and design docs | doc review | keep |
| `documents/` | Runbooks, security, test programs; `archive/` holds history | doc review | keep |
| `icons/` | App icons for both desktop shells | bundled by Tauri | keep |
| `legacy/` | Retired single-agent app as a standalone workspace | not in CI | keep isolated |
| `npm-package/` | `abigail-desktop` npm installer wrapper | release publish step | keep |
| `scripts/` | CI guards, release, signing, dev launch, acceptance | `check_stability`, signing checks | keep |
| `skills/` | Skill crates, manifests, and registry | workspace check/test | keep |
| `templates/` | Constitutional and runtime bootstrap content | runtime use | keep |
| `target/`, `output/` | Local build/generated output | regenerated | keep ignored |

## Workspace Members

### Applications and daemons

| Member | Role | Coverage / Validation |
| --- | --- | --- |
| `abigail-hive-app` | Hive desktop shell; starts `hive-daemon` and opens Runtime windows | `cargo check`, frontend tests |
| `abigail-entity-runtime-app` | Entity chat window bundled inside the Hive installer | `cargo check`, frontend tests |
| `hive-daemon` | Control plane: Entity registry, birth rite, signing, providers, secrets, supervisor, store owner | HTTP contract + cold-start tests |
| `entity-daemon` | One Entity's runtime: chat, routing, skills, memory, Superego | HTTP contract tests |
| `hive-cli` | Operator CLI for `hive-daemon` | workspace compile |
| `entity-cli` | Operator CLI for `entity-daemon` | workspace compile |
| `abigail-cli` | Standalone troubleshooting CLI and REST server | workspace compile |
| `abigail-keygen` | Desktop utility for generating master and identity keys | workspace compile |

### Framework crates

| Member | Role | Used by |
| --- | --- | --- |
| `abigail-identity` | Entity registry, Hive master key, birth signing, fail-closed load verification | `hive-daemon`, `entity-daemon` |
| `soul-forge` | Soul Forge ethical trials, Triangle Ethic weights, soul hash | `hive-daemon`, `entity-daemon`, `abigail-birth` |
| `abigail-birth` | Conversational birth orchestrator and genesis paths | none in the workspace (only `legacy/tauri-app`) |
| `abigail-soul-crystallization` | LLM-guided soul crystallization | `abigail-birth` |
| `abigail-memory` | Conversation turns, memories, archives, backup import | `entity-daemon`, `entity-chat`, `abigail-birth`, `abigail-soul-crystallization` |
| `abigail-persistence` | Embedded SurrealDB, `hive` / `entity_<UUID>` scopes, lease-scoped remote transport | both daemons, `abigail-memory`, `abigail-queue`, storage skills |
| `abigail-superego` | Ethical evaluation and output gating | `entity-daemon` |
| `abigail-router` | Id/Ego routing, agentic engine, subagents, mentor chat monitor | `entity-daemon`, `entity-chat`, `abigail-cli` |
| `abigail-skills` | Skill registry, execution, signed allowlist and confirmation policy | both daemons, `abigail-runtime`, `entity-chat`, skills |
| `abigail-hive` | Provider construction, secret resolution, installed-CLI detection | both daemons, `abigail-router` |
| `abigail-core` | Config, vault, keyring, constitutional verification, secure file I/O | nearly every crate |
| `abigail-capabilities` | Innate cognitive, sensory, and memory capabilities | both daemons, `abigail-router`, `entity-chat` |
| `abigail-runtime` | Skill package wiring | both daemons, `abigail-cli` |
| `entity-chat` | Shared chat engine | `entity-daemon`, `abigail-cli` |
| `abigail-queue` | Persistent sub-agent job queue on the embedded store | `entity-daemon`, `abigail-router`, `abigail-skills`, `entity-chat` |
| `abigail-streaming` | `StreamBroker` trait for topic messaging | `entity-daemon`, `abigail-router`, `abigail-skills`, observers |
| `abigail-id` | Id observer | `entity-daemon` |
| `abigail-auth` | Auth provider traits and token cache for skills | `abigail-skills` |
| `abigail-diag` | Startup file logging and panic hook | both daemons |
| `hive-core`, `entity-core` | DTO-only HTTP contracts | apps, daemons, CLIs, `daemon-client` |
| `daemon-client` | HTTP client used by the desktop shells | both desktop apps |
| `daemon-test-harness` | Starts both daemons for integration tests | daemon dev-dependencies |

### Skill packages

`skill-browser`, `skill-calendar`, `skill-clipboard`, `skill-code-analysis`,
`skill-database`, `skill-document`, `skill-filesystem`, `skill-git`,
`skill-http`, `skill-image`, `skill-knowledge-base`, `skill-notification`,
`skill-perplexity-search`, `skill-shell`, `skill-system-monitor`, and
`skill-web-search` are wired through `abigail-runtime` and covered by
workspace compile and tests.

## Isolated legacy

| Path | Status |
| --- | --- |
| `legacy/tauri-app/` (`abigail-app`) | Retired single-agent desktop app. Standalone Cargo workspace with its own lockfile, excluded from the root workspace, installers, and CI. Kept because it holds the only recovery-export UI and trusted-signer commands, and is the only consumer of `abigail-birth`. See [`legacy/README.md`](../legacy/README.md). |
| `documents/archive/legacy-tauri-app/` | Plans and reports written against the retired app. |

## Cleanup History

- Canonicalized the mentor monitor surface to `abigail_router::monitor::mentor_chat::*` and removed the deprecated `monitors::*` shim.
- Promoted the trust-paper alignment note into `documents/DECENTRALIZED_TRUST_PAPER_ALIGNMENT.md`.
- Deleted the dead `abigail/` subtree and the unreferenced `documents/README_ALTERNATIVE.md`.
- Moved the retired single-agent `tauri-app` to `legacy/tauri-app` as its own workspace; the split apps now take icons from `icons/` and Hive's default installer hooks from `scripts/nsis-mvp-hooks.nsh`.

## Deferred Sunset Notes

- Port recovery-bundle export and trusted-signer management into Hive, then retire the remaining legacy markers in `scripts/check_crypto_claims.mjs`.
- Decide whether Hive adopts `abigail-birth` / `abigail-soul-crystallization` for a conversational birth path or whether they move to `legacy/`.
- `entity-daemon` and `hive-daemon` integration tests run in CI behind `ABIGAIL_DAEMON_INTEGRATION=1`; keep them there rather than deleting them.
