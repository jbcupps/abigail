# Abigail Release Runbook

This repo has two repeatable GitHub Actions release lanes.

## Branch Channels

- `beta` is the permanent iteration and UAT branch. Merge implementation PRs there first.
- Every push to `beta` runs the one-step Windows installer workflow and publishes a GitHub prerelease tagged `vX.Y.Z-beta.N`.
- `main` is the promoted stable branch. Stable releases use clean `vX.Y.Z` tags and are not prereleases.

## Full Installer Release

Workflow: `Abigail Installer Release` (`.github/workflows/release.yml`)

Current active platforms:

- Windows x64 NSIS installer: `Abigail-windows-x64-setup.exe`
- Windows x64 MSI installer: `Abigail-windows-x64.msi`

The Windows installer is the family-facing release lane. It installs one `Abigail` app icon and bundles the internal split runtime pieces (`Abigail Hive`, `Abigail Entity Runtime`, `hive-daemon`, and `entity-daemon`) so users do not download separate binaries.

Run a beta UAT release from the permanent beta branch:

```bash
git push origin HEAD:beta
```

The workflow tags the build as `v<next-stable-version>-beta.<run-number>` and publishes it as a prerelease.

Linux one-step packaging is planned after the Windows lane is stable. macOS/Apple builds remain paused until the Apple Developer agreement/signing issue is resolved.

Run a specific release:

```bash
gh workflow run release.yml --ref main -f release_version=0.0.75
```

Run the next patch release automatically:

```bash
gh workflow run release.yml --ref main
```

Watch the newest run:

```bash
gh run watch --repo jbcupps/abigail
```

Verify the release:

```bash
gh release view v0.0.75 --json tagName,url,publishedAt,assets
```

The workflow creates the tag if it does not already exist, uploads stable Windows installer asset names to the GitHub Release, and publishes the release only after the installer build succeeds. `beta` builds are prereleases; `main` and clean `vX.Y.Z` tag builds are stable releases.

For `beta` releases, the workflow also downloads the published Windows installer back from the GitHub prerelease, verifies the expected internal split binaries are present, and uploads a `beta-uat-installer-verification` artifact containing the installer tag/version and inspected payload list.

Run Windows Family Beta UAT against a downloaded installer with Claude CLI system auth:

```powershell
pwsh ./scripts/uat/run-uat.ps1 -Provider claude-cli -InstallerPath .\Abigail-windows-x64-setup.exe -HivePort 3141
```

For source-level diagnostics without installing Abigail, omit `-InstallerPath`. The default provider is `claude-cli`; use `-Provider openai -KeysetFile scripts/uat/uat-keys.env` only for legacy API-key diagnostics.

## Repository Switches

The repeatable stabilization release path keeps signing and updater artifacts opt-in.

- `ABIGAIL_REQUIRE_WINDOWS_SIGNING=true` enables Windows signing checks and signing config.
- `ABIGAIL_WINDOWS_SIGNING_MODE=store` expects a certificate available on the Windows runner.
- `ABIGAIL_WINDOWS_RUNNER` can route Windows builds to a self-hosted runner label.
- `ABIGAIL_REQUIRE_UPDATER_SIGNING=true` enables Tauri updater artifacts and `latest.json`.
- `NPM_TOKEN` is optional. If present, the workflow attempts to publish `abigail-desktop`; if absent or the version already exists, GitHub release publishing still succeeds.

Current expected repeat-build posture:

- Leave `ABIGAIL_REQUIRE_WINDOWS_SIGNING` unset unless the signing machine is ready.
- Leave `ABIGAIL_REQUIRE_UPDATER_SIGNING` unset until the updater signing lane is intentionally restored.
- Keep Apple/macOS out of the build matrix until the Apple Developer account agreement/signing problem is fixed.
- Keep Linux out of the full installer matrix until the one-step Linux package is intentionally added.

## Unsigned Stabilization Build

Workflow: `Stabilization Build (Unsigned)` (`.github/workflows/release-fast.yml`)

Use it for quick Windows/Linux binary checks without installers, updater artifacts, signing, or Apple notarization:

```bash
gh workflow run release-fast.yml --ref main -f release_version=0.0.73 -f publish_prerelease=false
```

Set `publish_prerelease=true` only when you want those unsigned binaries published as a GitHub pre-release.

Publish the split Hive + Entity Runtime product as a diagnostic unsigned release only:

```bash
gh workflow run release-fast.yml --ref main -f release_version=0.0.74 -f publish_stable_release=true
```

The split product release uploads four side-by-side binaries for each platform:

- Abigail Hive app
- Abigail Entity Runtime app
- hive-daemon
- entity-daemon

This lane is for diagnostics and portable validation. Family-facing releases should use the full Windows installer lane so users install and launch one `Abigail` app.
