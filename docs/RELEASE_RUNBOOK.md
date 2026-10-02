# Abigail Release Runbook

This repo has two repeatable GitHub Actions release lanes.

Rust is pinned to the tested `1.97.0` compiler in `rust-toolchain.toml` and all
CI/release setup steps. The repository toolchain uses the minimal profile with
`rustfmt` and `clippy`; CodeQL autobuild and local Cargo commands also inherit
the repository pin. This avoids compiler drift between local validation and
hosted builds: floating `stable` advanced to Rust 1.99 and introduced Clippy
errors in `async_trait` generated code. Upgrade the pin only with coordinated
workspace tests, formatting, Clippy, and installer validation.

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

The workflow builds and tags the triggering commit. Both desktop shells embed their production frontend assets and the requested installer version. It refuses an existing tag that identifies another commit.

Before publishing an unsigned installer, the workflow installs it in a fresh test directory, checks all four payload hashes and Windows daemon loader checks, then runs the synthetic daemon contract against those installed binaries. The contract uses isolated data, Documents, and app profiles, exercises live streaming and durable history across restarts, and does not require a model account or API key. It is a contract check; it does not claim real-model inference. The installed verifier refuses to replace an existing Abigail installation outside the repository's test directory. Validation evidence is uploaded as `abigail-installer-validation-windows-latest`.

Publication also requires the `gate` job from the `CI` workflow to pass for the exact release commit on `beta` or `main`. The wait is bounded to one hour. Advisory audit or CodeQL results do not override a successful gate. `beta` builds are prereleases; promoted `main` and clean `vX.Y.Z` tag builds are stable releases. Windows signing and updater requirements remain opt-in.

For `beta` releases, the workflow also downloads the published Windows installer back from the GitHub prerelease, verifies the expected internal split binaries are present, and uploads a `beta-uat-installer-verification` artifact containing the installer tag/version and inspected payload list.

For current installed MVP acceptance, follow [Windows MVP validation](MVP_WINDOWS.md). Run the synthetic contract against the installed directory, then run the real-model harness with a model connection or an authenticated installed CLI. For example, an existing Codex account needs no API key:

```powershell
pwsh ./scripts/tests/run-mvp-acceptance.ps1 -BinaryDir <installed-directory>
node ./scripts/tests/mvp-real-model.mjs --binary-dir <installed-directory> --cli-provider codex-cli
```

The older `scripts/uat/run-uat.ps1` is a legacy Claude/OpenAI diagnostic and does not cover the current birth and private runtime bootstrap flow. It is not the release acceptance gate.

## Repository Switches

For existing SSL.com certificates, local signed builds, runner setup, and signature
verification, follow [Windows SSL.com signing](WINDOWS_SSL_COM_SIGNING.md).
The [Codex desktop handoff](CODEX_SSL_COM_HANDOFF.md) covers the account/provider steps.

The repeatable stabilization release path keeps signing and updater artifacts opt-in.

- `ABIGAIL_REQUIRE_WINDOWS_SIGNING=true` enables Windows signing checks and signing config.
- `ABIGAIL_WINDOWS_SIGNING_MODE=store` expects a certificate available on the Windows runner.
- `ABIGAIL_WINDOWS_RUNNER` can route Windows builds to a self-hosted runner label.
- `ABIGAIL_REQUIRE_UPDATER_SIGNING=true` enables Tauri updater artifacts and `latest.json`.

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

This older lane builds diagnostic binaries; it does not embed the current desktop frontends or preserve the internal executable names used by the installed app. Do not use `publish_stable_release` for a production release. Use the full installer workflow above for beta UAT and stable releases.

The legacy diagnostic command is:

```bash
gh workflow run release-fast.yml --ref main -f release_version=0.0.74 -f publish_stable_release=true
```

The split product release uploads four side-by-side binaries for each platform:

- Abigail Hive app
- Abigail Entity Runtime app
- hive-daemon
- entity-daemon

This lane is for diagnostics and portable validation. Family-facing releases should use the full Windows installer lane so users install and launch one `Abigail` app.
