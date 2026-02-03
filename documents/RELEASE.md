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

- Go to **Actions** → **build-release** → **Run workflow**.
- Optionally set **Release version** (e.g. `0.0.1`). If set, the workflow will build installers for Windows, Linux, and macOS and **publish** a release with that version (tag `v0.0.1`). If left empty, only the build runs and artifacts are kept in the run (no release).

## How to publish a release (tag-based)

1. **Bump version** (for 0.0.1 you’re already at 0.0.1; for the next release use 0.0.2):
   - In `Cargo.toml`: set `version = "0.0.2"` (or next number).
   - In `tauri-app/tauri.conf.json`: set `"version": "0.0.2"`.

2. **Commit and push** the version bump:
   ```bash
   git add Cargo.toml tauri-app/tauri.conf.json
   git commit -m "chore: release 0.0.2"
   git push origin master
   ```

3. **Create and push the tag** (triggers the release workflow):
   ```bash
   git tag v0.0.2
   git push origin v0.0.2
   ```

4. **CI:** `.github/workflows/build-release.yml` runs on `v*` tags:
   - Builds installers for Windows (NSIS), Linux (deb), and macOS (dmg).
   - Creates and **publishes** a GitHub Release for that tag with all installers attached (no manual publish step needed).

## Where to get installers (end users)

After a release is published, end users can go to the [Abby download page](https://jbcupps.github.io/abby/). The page detects their OS and offers a single download button; they can also pick Windows, macOS, or Linux from "Other downloads." Stable asset names (`Abby-windows-x64-setup.exe`, etc.) allow `.../releases/latest/download/...` URLs to always point at the latest release.

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
| 4 | Wait for build-release workflow to finish — release is published automatically. |
