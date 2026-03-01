# Release process

## Version scheme

- **Current:** `0.0.x` (first release is **0.0.1**).
- **Incremental:** Each new release bumps the **patch** segment: `0.0.1` → `0.0.2` → `0.0.3`, etc.
- When we introduce a new minor line (e.g. new features), we can move to `0.1.0` and continue patch bumps there.

Tags use a leading `v`: `v0.0.1`, `v0.0.2`, and so on.

## Where version is defined

Keep these in sync when cutting a release:

1. **Root `Cargo.toml`** — `[workspace.package]` → `version = "0.0.x"`
2. **`tauri-app/tauri.conf.json`** — `"version": "0.0.x"`

All workspace crates use `version.workspace = true`, so they follow the root.

## Build and release from GitHub Actions (no tag push)

- Go to **Actions** → **Release** → **Run workflow**.
- Optionally set **Release version** (e.g. `0.0.1`). If set, the workflow builds installers and publishes a release with that version (tag `v0.0.1`). If left empty, version is auto-incremented from the latest `v*` tag.

## How to publish a release (tag-based)

1. **Bump version** (for 0.0.1 you’re already at 0.0.1; for the next release use 0.0.2):
   - In `Cargo.toml`: set `version = "0.0.2"` (or next number).
   - In `tauri-app/tauri.conf.json`: set `"version": "0.0.2"`.

2. **Commit and push** the version bump:
   ```bash
   git add Cargo.toml tauri-app/tauri.conf.json
   git commit -m "chore: release 0.0.2"
   git push origin main
   ```

3. **Create and push the tag** (triggers the release workflow):
   ```bash
   git tag v0.0.2
   git push origin v0.0.2
   ```

4. **CI:** `.github/workflows/release.yml` runs on `v*` tags (two-stage: build matrix → publish):
   - Stage 1: Parallel builds for Windows (NSIS), Linux (deb), and macOS (dmg). Each job uploads installer artifacts.
   - Stage 2: Download artifacts, create GitHub Release with stable asset names, publish npm package. Proceeds if at least one platform built successfully.

## Where to get installers (end users)

After a release is published, end users can get Abigail through any of these channels:

### Direct download

Go to the [Abigail download page](https://jbcupps.github.io/abigail/). The page detects their OS and offers a single download button; they can also pick Windows, macOS, or Linux from "Other downloads." Stable asset names allow `.../releases/latest/download/...` URLs to always point at the latest release:

- `https://github.com/jbcupps/abigail/releases/latest/download/Abigail-windows-x64-setup.exe`
- `https://github.com/jbcupps/abigail/releases/latest/download/Abigail-macos-universal.dmg`
- `https://github.com/jbcupps/abigail/releases/latest/download/Abigail-linux-x64.deb`

### npm CLI

Install with a single command (requires Node.js 18+):

```bash
npx abigail-desktop
```

This detects your OS, downloads the correct installer, and runs it. See `npm-package/README.md` for all commands.

### Docker (development)

For building/developing Abigail in a container:

```bash
docker compose -f docker/docker-compose.yml up -d abigail-dev
docker compose -f docker/docker-compose.yml exec abigail-dev bash
# Inside container: cargo build && cargo test --all
```

See `documents/HOW_TO_RUN_LOCALLY.md` for full Docker development instructions.

## Daemon binaries

In addition to the Tauri desktop installer, the workspace produces standalone daemon binaries:

| Binary | Purpose | Build command |
|--------|---------|---------------|
| `hive-daemon` | Hive control plane HTTP server | `cargo build -p hive-daemon --release` |
| `entity-daemon` | Entity agent runtime HTTP server | `cargo build -p entity-daemon --release` |
| `hive-cli` | CLI client for Hive | `cargo build -p hive-cli --release` |
| `entity-cli` | CLI client for Entity | `cargo build -p entity-cli --release` |

These are not yet included in the automated release pipeline. Future releases will add them as standalone artifacts alongside the desktop installer.

## Platform-specific notes

| Platform | Installer | Notes |
|----------|-----------|-------|
| Windows (x64) | `Abigail-windows-x64-setup.exe` | NSIS installer, user-level (no admin) |
| macOS (Intel + Apple Silicon) | `Abigail-macos-universal.dmg` | Universal binary. Not notarized -- right-click > Open on first launch |
| Ubuntu/Debian (x64) | `Abigail-linux-x64.deb` | Requires `libwebkit2gtk-4.1-0`, `libayatana-appindicator3-1`. Ubuntu 22.04+ |

## First release (0.0.1)

- Version is already set to **0.0.1** in this repo.
- To publish release **0.0.1**: create tag `v0.0.1` and push:
  ```bash
  git tag v0.0.1
  git push origin v0.0.1
  ```
- After the workflow completes, the release is published automatically and visible in **Releases**.

## Incremental checklist (each release)

| Step | Action |
|------|--------|
| 1 | Bump `version` in `Cargo.toml` and `tauri-app/tauri.conf.json` to next patch (e.g. 0.0.2). |
| 2 | Commit and push the version bump. |
| 3 | `git tag v0.0.x` and `git push origin v0.0.x`. |
| 4 | Wait for **Release** workflow to finish — release is published automatically. |

## Stability gates for GUI/Entity message-flow program

For releases in the current stabilization window, also require the gates in:

- `documents/GUI_ENTITY_STABILITY_ROADMAP.md`
- `documents/GUI_ENTITY_CODE_REVIEW_REPORT.md`

Minimum required before release `Go`:

1. Command Surface Gate: frontend command usage matches registered Tauri handlers.
2. Chat Parity Gate: transport adapters produce equivalent chat/session/trace behavior.
3. Agent Lifecycle Gate: run lifecycle (start/ask/confirm/cancel/complete/recover) is green.
4. Policy Gate: required trust/policy checks are runtime-enforced and tested.
5. No open P0 defects in GUI chat/entity-agent stability scope.

## npm publishing

The `abigail-desktop` npm package is published automatically by the **Release** workflow (Stage 2):

1. After the build stage completes, the publish job downloads installer artifacts, creates the GitHub Release, then updates `npm-package/package.json` with the release version and runs `npm publish`.
2. Requires `NPM_TOKEN` secret in **Settings > Secrets and variables > Actions**.

To set up npm publishing:
1. Create an npm access token at [npmjs.com](https://www.npmjs.com/settings/~/tokens).
2. Add it as a repository secret named `NPM_TOKEN` in **Settings > Secrets and variables > Actions**.

To republish npm only, run the **Release** workflow manually with the desired version (npm publish runs in the same job as the GitHub Release).
