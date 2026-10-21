# How to Run Abigail Locally

Abigail is a framework for creating and hosting persistent AI Entities. Locally you run the same pieces the installer ships:

- `hive-daemon` (control plane) and the `hive-app` desktop shell. Hive creates, births, and signs Entities, owns `memory.db`, and supervises runtimes.
- `entity-daemon` (one per open Entity) and the `entity-runtime-app` chat window. Each Entity uses its own `entity_<UUID>` database inside the Hive-owned store.

The retired single-agent app is in [`legacy/tauri-app`](../legacy/README.md) and is not covered here.

## Prerequisites

### Required (native development)

- Rust (the version pinned in `rust-toolchain.toml`)
- Node.js 20+ and `npm`
- Platform dependencies for Tauri:
  - **Windows**: NSIS (for installer builds)
  - **macOS**: Xcode Command Line Tools
  - **Ubuntu 22.04+**: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev libgtk-3-dev libxdo-dev`

### Required (Docker development)

- Docker 20+ and Docker Compose v2. The `docker/` directory contains `Dockerfile` and `docker-compose.yml` for containerized build and test.

### Optional

- A local OpenAI-compatible endpoint (Ollama, LM Studio, or another local server)
- Provider API keys, or an installed official Claude, Codex, or Grok CLI signed in with your own account. All of these are connected in Hive.

## Quick start (desktop)

From the repository root:

```bash
npm ci --prefix hive-app/src-ui
npm ci --prefix entity-runtime-app/src-ui

# Hive finds these next to its own executable.
cargo build -p hive-daemon -p entity-daemon -p abigail-entity-runtime-app

# Debug Entity Runtime windows load their UI from this dev server.
npm run dev --prefix entity-runtime-app/src-ui &

cd hive-app && cargo tauri dev
```

Hive starts (or adopts) its daemon, then opens an Entity Runtime window when you open an Entity. If `cargo tauri` is missing, install it with `cargo install tauri-cli`.

## Quick start (headless daemons)

Entity runtimes only start when Hive authorizes them, so run Hive and let it open Entities:

```bash
# Terminal 1: Hive control plane (default port 43141)
cargo build -p hive-daemon -p entity-daemon
cargo run -p hive-daemon -- --data-dir ./target/dev-data

# Terminal 2: create an Entity, then ask Hive to start its runtime
cargo run -p hive-cli -- create "MyEntity"
curl -X POST http://127.0.0.1:43141/v1/entities/<uuid>/open   # returns the runtime's local_url

# Talk to that runtime
cargo run -p entity-cli -- --url <local_url> status
cargo run -p entity-cli -- --url <local_url> chat "hello"
cargo run -p hive-cli -- entities
```

Starting `entity-daemon` by hand fails with "Open this Entity through the Abigail Hive to authorize its runtime": Hive passes each runtime a one-use bootstrap token, and the runtime exchanges it for a lease that scopes its persistence calls to its own Entity database.

### Daemon flags

**hive-daemon:**
- `--port <PORT>`: listen port (default `43141`)
- `--data-dir <PATH>`: data root (default: platform app data dir)
- `--doctor`: run each startup step non-fatally, print `[OK]`/`[FAIL]`, and exit

**entity-daemon** (normally set by the Hive supervisor):
- `--entity-id <UUID>`: registered Entity UUID
- `--hive-url <URL>`: Hive daemon URL (default `http://127.0.0.1:43141`)
- `--port <PORT>`: listen port (default `43142`)
- `--data-dir <PATH>`: must match Hive's `--data-dir`

`hive-cli` and `entity-cli` default to `http://127.0.0.1:43141` and `http://127.0.0.1:43142`; pass `--url` to override.

## Browser harness

`dev-harness/` is a static browser UI that talks directly to the Hive and Runtime daemon APIs. It is the fallback when desktop builds are blocked (for example by Windows Application Control):

```bash
node ./scripts/dev/run_browser_harness.mjs --hive-url http://127.0.0.1:43141 --runtime-url <local_url>
```

It serves on `http://127.0.0.1:43143`.

## Quick start (Docker)

Use the development container to avoid installing Rust, Node.js, and system dependencies on your host:

```bash
docker compose -f docker/docker-compose.yml build
docker compose -f docker/docker-compose.yml up -d abigail-dev
docker compose -f docker/docker-compose.yml exec abigail-dev bash

# Inside the container:
npm ci --prefix hive-app/src-ui && npm ci --prefix entity-runtime-app/src-ui
cargo build
cargo test --workspace
```

The container bind-mounts the repo, so host edits appear immediately. Desktop GUIs need X11/Wayland forwarding, which is not covered here; use Docker for building and testing.

To run a one-shot build and test in an isolated container:

```bash
docker compose -f docker/docker-compose.yml run --rm abigail-build
```

## Runtime data layout

Everything lives under the Hive data root (the OS app-data directory, or `--data-dir`):

- `memory.db`: the embedded SurrealDB store. Hive records are in `abigail/hive`; each Entity's conversations, memories, and jobs are in `abigail/entity_<UUID>` (hyphens become underscores).
- `identities/<UUID>/`: one directory per Entity with `config.json`, `external_pubkey.bin`, the encrypted `birth_signing_key.vault`, and `docs/` (constitutional documents, their signatures, soul documents, and `birth_certificate.json`).
- `skill_secrets/` and `entity_secrets/`: encrypted secret vaults.

Legacy SQLite files (`abigail_seed.db`, `abigail_memory.db`, `jobs.db`, `calendar.db`, `kb.db`) are read only as first-launch migration inputs.

## Entity lifecycle

1. **Create** (`POST /v1/entities`): Hive registers a UUID and identity directory.
2. **Birth** (`POST /v1/entities/:id/birth`): the Soul Forge rite runs three ethical trials, or a quickstart path, to produce Triangle Ethic weights and soul documents.
3. **Sign**: Hive generates the Entity's Ed25519 key, signs its constitutional documents, countersigns its public key with the Hive master key, and writes a Hive-signed birth certificate.
4. **Open** (`POST /v1/entities/:id/open`): Hive starts the Entity's `entity-daemon`. Loading a born Entity fails closed if the signature chain or constitution does not verify.

The Entity's private signing key is stored encrypted in its identity directory (`birth_signing_key.vault`); there is no separate key to copy down during setup.

## Provider configuration

Provider and model selection belongs to Hive, never to an individual Entity:

- **In Hive**: connect a local server, store a cloud provider key in the Hive vault, or select an installed official CLI. Hive verifies a real model response before saving a CLI connection.
- **Environment variables** for development: see [`example.env`](../example.env).

## Installer builds

- Unsigned Windows MVP: `pwsh ./scripts/build-mvp-windows.ps1 -VerifyInstall`. See [`docs/MVP_WINDOWS.md`](../docs/MVP_WINDOWS.md).
- Release and signing lanes: [`docs/RELEASE_RUNBOOK.md`](../docs/RELEASE_RUNBOOK.md) and [`RELEASE.md`](RELEASE.md).

## Validation commands

Run from the repo root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo check -p abigail-hive-app -p abigail-entity-runtime-app
(cd hive-app/src-ui && npm run build && npm test)
(cd entity-runtime-app/src-ui && npm run build && npm test)
```

Daemon HTTP contract tests are gated by `ABIGAIL_DAEMON_INTEGRATION=1` and need the daemon binaries built first:

```bash
cargo build -p hive-daemon -p entity-daemon
ABIGAIL_DAEMON_INTEGRATION=1 cargo test -p hive-daemon --test integration
ABIGAIL_DAEMON_INTEGRATION=1 cargo test -p entity-daemon --test integration
```

Some provider and skill tests need credentials, feature flags, or external services.
