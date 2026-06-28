# Abigail Release Runbook

This repo has two repeatable GitHub Actions release lanes.

## Full Installer Release

Workflow: `Beta Release (Signed)` (`.github/workflows/release.yml`)

Current active platforms:

- Windows x64 NSIS installer: `Abigail-windows-x64-setup.exe`
- Windows x64 MSI installer: `Abigail-windows-x64.msi`
- Ubuntu 22.04+ x64 Debian package: `Abigail-linux-x64.deb`

macOS/Apple builds are temporarily removed from the active build matrix. The dormant macOS steps remain in the workflow so the lane can be restored later by re-adding the macOS matrix entry after the Apple Developer agreement/signing issue is resolved.

Run a specific release:

```bash
gh workflow run release.yml --ref main -f release_version=0.0.73
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
gh release view v0.0.73 --json tagName,url,publishedAt,assets
```

The workflow creates the `vX.Y.Z` tag if it does not already exist, uploads stable asset names to the GitHub Release, and publishes the release only after both active platform builds succeed.

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

## Unsigned Stabilization Build

Workflow: `Stabilization Build (Unsigned)` (`.github/workflows/release-fast.yml`)

Use it for quick Windows/Linux binary checks without installers, updater artifacts, signing, or Apple notarization:

```bash
gh workflow run release-fast.yml --ref main -f release_version=0.0.73 -f publish_prerelease=false
```

Set `publish_prerelease=true` only when you want those unsigned binaries published as a GitHub pre-release.
